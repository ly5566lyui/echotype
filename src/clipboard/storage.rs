use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing::info;

pub struct ClipboardStorage {
    conn: Arc<Mutex<Connection>>,
}

impl ClipboardStorage {
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let conn = Connection::open(db_path.as_ref())
            .with_context(|| format!("Failed to open SQLite database at {:?}", db_path.as_ref()))?;

        // 多进程并发访问 (GUI + daemon 同时运行) 需要 WAL 模式与写锁等待，
        // 否则 SQLITE_BUSY 会导致历史入库静默失败
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;

        // 初始化数据表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS clipboard_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content TEXT NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS quick_phrases (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                shortcut TEXT NOT NULL UNIQUE,
                phrase TEXT NOT NULL,
                description TEXT
            )",
            [],
        )?;

        info!("Clipboard and quick phrases database initialized.");
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// 插入一条新的剪贴板记录
    pub fn insert_history(&self, content: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO clipboard_history (content) VALUES (?1)",
            params![content],
        )?;
        Ok(())
    }

    /// 获取最近的 N 条剪贴板历史
    pub fn get_recent_history(&self, limit: usize) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT content FROM clipboard_history ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| row.get(0))?;
        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    /// 按上限裁剪剪贴板历史，仅保留最近 max_items 条 (使 config 的 max_history_items 生效)
    pub fn trim_history(&self, max_items: usize) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM clipboard_history WHERE id NOT IN (
                SELECT id FROM clipboard_history ORDER BY id DESC LIMIT ?1
            )",
            params![max_items as i64],
        )?;
        Ok(())
    }

    /// 添加自定义常用短语
    pub fn add_quick_phrase(&self, shortcut: &str, phrase: &str, desc: Option<&str>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO quick_phrases (shortcut, phrase, description) VALUES (?1, ?2, ?3)",
            params![shortcut, phrase, desc],
        )?;
        Ok(())
    }

    /// 读取全部常用短语 (设置窗口展示 / 上屏选择)
    pub fn get_quick_phrases(&self, limit: usize) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT phrase FROM quick_phrases ORDER BY id ASC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| row.get(0))?;
        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    /// 删除一条常用语 (按内容匹配，删除最早的一条)
    pub fn delete_quick_phrase(&self, phrase: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM quick_phrases WHERE id = (SELECT id FROM quick_phrases WHERE phrase = ?1 LIMIT 1)",
            params![phrase],
        )?;
        Ok(())
    }

    /// 用设置窗口编辑后的列表整体覆盖常用短语库
    pub fn replace_quick_phrases(&self, phrases: &[String]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM quick_phrases", [])?;
        for (i, phrase) in phrases.iter().enumerate() {
            if phrase.trim().is_empty() {
                continue;
            }
            conn.execute(
                "INSERT OR REPLACE INTO quick_phrases (shortcut, phrase, description) VALUES (?1, ?2, ?3)",
                params![format!("p_{:03}", i), phrase, None::<&str>],
            )?;
        }
        info!("Replaced quick phrase library ({} entries).", phrases.len());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_phrases_roundtrip() {
        let storage = ClipboardStorage::new(":memory:").unwrap();
        storage
            .replace_quick_phrases(&["第一条".into(), "  ".into(), "第二条".into()])
            .unwrap();
        let list = storage.get_quick_phrases(10).unwrap();
        // 空白项应被过滤，顺序保持插入序
        assert_eq!(list, vec!["第一条".to_string(), "第二条".to_string()]);
    }

    #[test]
    fn history_trim_keeps_latest() {
        let storage = ClipboardStorage::new(":memory:").unwrap();
        for i in 0..10 {
            storage.insert_history(&format!("item-{i}")).unwrap();
        }
        storage.trim_history(3).unwrap();
        let recent = storage.get_recent_history(10).unwrap();
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0], "item-9"); // 最新的在前
    }
}
