use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use anyhow::{bail, Context, Result};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::config::P2pSyncConfig;

/// mDNS 服务类型 (局域网内设备通过它互相发现)
const SERVICE_TYPE: &str = "_echotype._tcp.local.";
/// 同步协议版本
const PROTOCOL_VERSION: u8 = 1;
/// 单条同步消息上限 (1 MiB)，超出直接丢弃，防止恶意/异常流量
const MAX_FRAME_LEN: usize = 1024 * 1024;
/// 消息时间戳重放保护窗口 (秒)：偏离本机时钟超过该值的帧直接丢弃，
/// 防止嗅探到的历史密文帧被无限重放 (裸 AES-GCM 不像 TLS 那样自带防重放)
const REPLAY_WINDOW_SECS: u64 = 30;
/// 同时处理的入站连接上限 (防止异常连接耗尽本机资源)
const MAX_CONCURRENT_CONNECTIONS: usize = 16;
/// 远端内容写入本地剪贴板后，监听线程会将其视为新的本地内容回调；
/// 在此时长内若内容与最近一次远端内容一致，则不再转发 (抑制 Echo 风暴)
const ECHO_SUPPRESS_SECS: u64 = 10;

/// P2P 同步的消息负载 (经 AES-256-GCM 加密后传输)
#[derive(Debug, Serialize, Deserialize)]
struct P2pClipboardMessage {
    device: String,
    content: String,
    ts: u64,
}

/// 局域网跨设备剪贴板同步管理器
///
/// - 发现：mDNS 注册 `_echotype._tcp.local.` 服务并浏览对端
/// - 传输：TCP 长度前缀帧
/// - 加密：pair_secret 经 SHA-256 派生密钥，AES-256-GCM 认证加密
pub struct P2pSyncManager {
    device_name: String,
    instance: String,
    key: [u8; 32],
    port: u16,
    /// 本机 mDNS 实例全名，用于在浏览结果中过滤自己
    fullname: String,
    inner: Arc<P2pInner>,
}

struct P2pInner {
    /// 已发现的局域网对端: mDNS 实例全名 -> 可达地址列表
    peers: Mutex<HashMap<String, Vec<SocketAddr>>>,
    /// 最近一次收到的远端内容 (用于抑制监听线程 Echo)
    last_remote: Mutex<Option<(String, Instant)>>,
    /// 待广播内容队列 (广播方非阻塞投递，由独立任务统一发送)
    outbound: mpsc::UnboundedSender<String>,
}

/// 当前活动的同步实例 (GUI 单例场景下供设置窗口查询对端状态)
static ACTIVE_INSTANCE: Mutex<Option<Arc<P2pSyncManager>>> = Mutex::new(None);

/// 获取当前活动的 P2P 同步实例 (未启动时为 None)
pub fn active_instance() -> Option<Arc<P2pSyncManager>> {
    ACTIVE_INSTANCE.lock().ok().and_then(|g| g.clone())
}

impl P2pSyncManager {
    /// 启动 P2P 同步 (注册并浏览 mDNS、开启 TCP 服务端、启动发送泵)。
    /// 必须在 tokio 运行时上下文中调用。
    pub async fn start(cfg: &P2pSyncConfig) -> Result<Arc<Self>> {
        if cfg.pair_secret.trim().is_empty() {
            bail!("pair_secret is empty; refusing to sync plaintext clipboard over LAN.");
        }

        let key = derive_key(&cfg.pair_secret);
        let instance = sanitize_instance(&cfg.device_name);
        let fullname = format!("{}.{}", instance, SERVICE_TYPE);

        let (tx, rx) = mpsc::unbounded_channel::<String>();
        let inner = Arc::new(P2pInner {
            peers: Mutex::new(HashMap::new()),
            last_remote: Mutex::new(None),
            outbound: tx,
        });

        let manager = Arc::new(Self {
            device_name: cfg.device_name.clone(),
            instance,
            key,
            port: cfg.port,
            fullname,
            inner,
        });

        // 登记为全局活动实例 (供设置窗口查询)
        if let Ok(mut g) = ACTIVE_INSTANCE.lock() {
            *g = Some(Arc::clone(&manager));
        }

        // 1. mDNS 注册 + 对端发现 (阻塞式 API，放到独立线程池)
        {
            let mgr = Arc::clone(&manager);
            let display = cfg.device_name.clone();
            tokio::task::spawn_blocking(move || {
                if let Err(e) = mdns_register_and_browse(mgr, &display) {
                    error!("mDNS discovery stopped: {:#}", e);
                }
            });
        }

        // 2. TCP 接收端
        {
            let mgr = Arc::clone(&manager);
            tokio::spawn(async move {
                if let Err(e) = tcp_server(mgr).await {
                    error!("P2P sync TCP server stopped: {:#}", e);
                }
            });
        }

        // 3. 广播发送泵
        {
            let mgr = Arc::clone(&manager);
            tokio::spawn(async move {
                outbound_pump(mgr, rx).await;
            });
        }

        info!(
            "P2P clipboard sync started: device '{}', instance '{}', port {}",
            manager.device_name, manager.fullname, manager.port
        );
        Ok(manager)
    }

    /// 将本地剪贴板内容加密广播给所有已发现的对端 (非阻塞，内部排队发送)
    pub fn broadcast(&self, content: &str) {
        if content.trim().is_empty() {
            return;
        }
        {
            let guard = self.inner.last_remote.lock().unwrap();
            if let Some((last, at)) = guard.as_ref() {
                if last == content && at.elapsed() < Duration::from_secs(ECHO_SUPPRESS_SECS) {
                    debug!("Suppressing P2P echo of remote clipboard content.");
                    return;
                }
            }
        }
        if let Err(e) = self.inner.outbound.send(content.to_string()) {
            debug!("P2P outbound queue closed: {}", e);
        }
    }

    /// 当前已发现的对端数量 (诊断/状态展示用)
    #[allow(dead_code)] // 预留: 供后续状态浮层/CLI 诊断使用
    pub fn peer_count(&self) -> usize {
        self.inner.peers.lock().unwrap().len()
    }
}

/// 在独立线程中注册本机服务并持续浏览对端
fn mdns_register_and_browse(manager: Arc<P2pSyncManager>, device_display: &str) -> Result<()> {
    let mdns = ServiceDaemon::new().context("failed to start mDNS daemon")?;

    // 注册本机服务；enable_addr_auto 自动携带本机全部网卡地址
    let host = format!("{}.local.", manager.instance);
    let info = ServiceInfo::new(SERVICE_TYPE, &manager.instance, &host, "", manager.port, None)
        .with_context(|| {
            format!(
                "invalid mDNS service info (device '{}')",
                device_display
            )
        })?
        .enable_addr_auto();
    mdns.register(info)
        .context("failed to register mDNS service")?;
    info!("mDNS service registered: {}", manager.fullname);

    let receiver = mdns
        .browse(SERVICE_TYPE)
        .context("failed to browse mDNS services")?;

    loop {
        match receiver.recv() {
            Ok(event) => match event {
                ServiceEvent::ServiceResolved(info) => {
                    let fullname = info.get_fullname().to_string();
                    if fullname == manager.fullname {
                        continue; // 忽略自己
                    }
                    let port = info.get_port();
                    let addrs: Vec<SocketAddr> = info
                        .get_addresses()
                        .iter()
                        .map(|ip| SocketAddr::new(*ip, port))
                        .collect();
                    if addrs.is_empty() {
                        continue;
                    }
                    let mut peers = manager.inner.peers.lock().unwrap();
                    if peers.insert(fullname.clone(), addrs).is_none() {
                        info!("P2P peer discovered: {}", fullname);
                    }
                }
                ServiceEvent::ServiceRemoved(_ty, fullname) => {
                    manager.inner.peers.lock().unwrap().remove(&fullname);
                    info!("P2P peer removed: {}", fullname);
                }
                _ => {}
            },
            Err(e) => {
                // recv 仅在 mDNS 守护线程退出 (channel 关闭) 时返回 Err，属于致命错误
                bail!("mDNS browse receiver closed: {}", e);
            }
        }
    }
}

async fn tcp_server(manager: Arc<P2pSyncManager>) -> Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", manager.port))
        .await
        .with_context(|| format!("failed to bind P2P sync port {}", manager.port))?;
    info!("P2P sync TCP server listening on 0.0.0.0:{}", manager.port);

    let active: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));

    loop {
        let (mut stream, peer) = match listener.accept().await {
            Ok(v) => v,
            Err(e) => {
                warn!("P2P accept failed: {}", e);
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }
        };

        if active.load(Ordering::SeqCst) >= MAX_CONCURRENT_CONNECTIONS {
            debug!(
                "P2P connection limit ({}) reached, dropping {}",
                MAX_CONCURRENT_CONNECTIONS, peer
            );
            continue;
        }
        active.fetch_add(1, Ordering::SeqCst);

        let mgr = Arc::clone(&manager);
        let active_task = Arc::clone(&active);
        tokio::spawn(async move {
            if let Err(e) = handle_incoming(mgr, &mut stream, peer).await {
                debug!("Dropping P2P connection from {}: {:#}", peer, e);
            }
            active_task.fetch_sub(1, Ordering::SeqCst);
        });
    }
}

async fn handle_incoming(
    manager: Arc<P2pSyncManager>,
    stream: &mut TcpStream,
    peer: SocketAddr,
) -> Result<()> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len == 0 || len > MAX_FRAME_LEN {
        bail!("invalid frame length {}", len);
    }

    let mut frame = vec![0u8; len];
    stream.read_exact(&mut frame).await?;

    // 安全事件使用 warn 级，保证默认日志级别下可见 (拒绝原因/来源对排查至关重要)
    let plaintext = match open_sealed(&manager.key, &frame) {
        Ok(p) => p,
        Err(e) => {
            warn!("P2P rejected frame from {}: {:#}", peer, e);
            bail!("decryption rejected");
        }
    };
    let msg: P2pClipboardMessage = match serde_json::from_slice(&plaintext) {
        Ok(m) => m,
        Err(e) => {
            warn!("P2P rejected malformed payload from {}: {}", peer, e);
            bail!("payload rejected");
        }
    };

    // 重放保护: 拒绝时间戳偏离本机时钟过远的消息
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if !is_timestamp_fresh(msg.ts, now) {
        warn!(
            "P2P rejected replay/stale timestamp from {} (ts={}, now={})",
            peer, msg.ts, now
        );
        bail!("stale or future message timestamp (replay rejected)");
    }

    if msg.content.is_empty() {
        return Ok(());
    }

    info!(
        "P2P clipboard received from '{}' ({} chars).",
        msg.device,
        msg.content.chars().count()
    );

    // 标记为远端来源，抑制本机监听线程将其 Echo 回传
    {
        let mut last = manager.inner.last_remote.lock().unwrap();
        *last = Some((msg.content.clone(), Instant::now()));
    }

    // 写入系统剪贴板；历史入库由 ClipboardMonitor 检测到变化后统一处理
    let content = msg.content;
    let _ = tokio::task::spawn_blocking(move || {
        if let Ok(mut cb) = arboard::Clipboard::new() {
            let _ = cb.set_text(content);
        }
    })
    .await;
    Ok(())
}

async fn outbound_pump(manager: Arc<P2pSyncManager>, mut rx: mpsc::UnboundedReceiver<String>) {
    while let Some(content) = rx.recv().await {
        let peers: Vec<SocketAddr> = {
            let guard = manager.inner.peers.lock().unwrap();
            guard.values().flatten().copied().collect()
        };
        if peers.is_empty() {
            debug!("P2P broadcast skipped: no peers discovered yet.");
            continue;
        }

        let msg = P2pClipboardMessage {
            device: manager.device_name.clone(),
            content,
            ts: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        let payload = match serde_json::to_vec(&msg) {
            Ok(v) => v,
            Err(e) => {
                warn!("P2P serialize failed: {}", e);
                continue;
            }
        };
        let frame = match seal(&manager.key, &payload) {
            Ok(f) => f,
            Err(e) => {
                warn!("P2P encrypt failed: {:#}", e);
                continue;
            }
        };

        for addr in peers {
            let frame = frame.clone();
            tokio::spawn(async move {
                if let Err(e) = send_frame(addr, &frame).await {
                    debug!("P2P send to {} failed: {:#}", addr, e);
                }
            });
        }
    }
}

async fn send_frame(addr: SocketAddr, frame: &[u8]) -> Result<()> {
    let send = async {
        let mut stream = TcpStream::connect(addr).await?;
        stream.write_all(&(frame.len() as u32).to_be_bytes()).await?;
        stream.write_all(frame).await?;
        stream.shutdown().await?;
        Ok::<(), anyhow::Error>(())
    };
    match tokio::time::timeout(Duration::from_secs(3), send).await {
        Ok(inner) => inner,
        Err(_) => bail!("timeout sending frame to {}", addr),
    }
}

/// 由预共享配对口令派生 AES-256 密钥 (加盐域隔离，避免与其他用途的口令哈希重合)
fn derive_key(pair_secret: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"echotype-p2p-v1:");
    hasher.update(pair_secret.trim().as_bytes());
    hasher.finalize().into()
}

/// 时间戳新鲜度判定 (±REPLAY_WINDOW_SECS 容忍双端时钟偏差)
fn is_timestamp_fresh(ts: u64, now: u64) -> bool {
    let lower = now.saturating_sub(REPLAY_WINDOW_SECS);
    let upper = now.saturating_add(REPLAY_WINDOW_SECS);
    ts >= lower && ts <= upper
}

/// 加密封帧: [version u8][nonce 12B][AES-256-GCM 密文]
fn seal(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(key).context("invalid AES-256 key length")?;
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|e| anyhow::anyhow!("AES-GCM encrypt failed: {}", e))?;
    let mut frame = Vec::with_capacity(1 + nonce.len() + ciphertext.len());
    frame.push(PROTOCOL_VERSION);
    frame.extend_from_slice(&nonce);
    frame.extend_from_slice(&ciphertext);
    Ok(frame)
}

fn open_sealed(key: &[u8; 32], frame: &[u8]) -> Result<Vec<u8>> {
    if frame.len() < 1 + 12 {
        bail!("frame too short");
    }
    if frame[0] != PROTOCOL_VERSION {
        bail!("unsupported protocol version {}", frame[0]);
    }
    let cipher = Aes256Gcm::new_from_slice(key).context("invalid AES-256 key length")?;
    let nonce = Nonce::from_slice(&frame[1..13]);
    let plaintext = cipher
        .decrypt(nonce, &frame[13..])
        .map_err(|_| anyhow::anyhow!("AES-GCM decrypt failed (wrong key or corrupted)"))?;
    Ok(plaintext)
}

/// 将设备名清洗为合法的 mDNS 实例名，并追加随机后缀保证局域网内唯一
fn sanitize_instance(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .collect();
    let trimmed = cleaned.trim_matches('-');
    let base = if trimmed.is_empty() {
        "device".to_string()
    } else {
        trimmed.to_string()
    };
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    format!("{}-{:x}", base, nanos & 0xffff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_open_roundtrip() {
        let key = derive_key("shared-secret");
        // 注意: 字节串字面量只允许 ASCII，测试负载不能包含非 ASCII 字符
        let payload = br#"{"device":"a","content":"hello-cai","ts":1}"#;
        let frame = seal(&key, payload).expect("seal failed");
        assert_eq!(frame[0], PROTOCOL_VERSION);
        let opened = open_sealed(&key, &frame).expect("open failed");
        assert_eq!(opened.as_slice(), &payload[..]);
    }

    #[test]
    fn open_rejects_wrong_key() {
        let frame = seal(&derive_key("secret-a"), b"hello").expect("seal failed");
        assert!(open_sealed(&derive_key("secret-b"), &frame).is_err());
    }

    #[test]
    fn open_rejects_tampered_ciphertext() {
        let mut frame = seal(&derive_key("secret"), b"clipboard text").expect("seal failed");
        let last = frame.len() - 1;
        frame[last] ^= 0xff;
        assert!(open_sealed(&derive_key("secret"), &frame).is_err());
    }

    #[test]
    fn instance_name_is_sanitized() {
        let name = sanitize_instance("My 桌面 Device!");
        assert!(name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
        assert!(!name.is_empty());
    }

    #[test]
    fn replay_window_accepts_fresh_and_rejects_stale() {
        let now: u64 = 1_700_000_000;
        assert!(is_timestamp_fresh(now, now));
        assert!(is_timestamp_fresh(now - 29, now)); // 略旧的时钟偏差容忍
        assert!(is_timestamp_fresh(now + 29, now));
        assert!(!is_timestamp_fresh(now - 31, now)); // 过旧 -> 重放拒绝
        assert!(!is_timestamp_fresh(now + 31, now)); // 未来 -> 异常拒绝
        assert!(!is_timestamp_fresh(0, now)); // 饱和边界
    }
}
