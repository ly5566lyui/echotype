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

    /// 添加自定义常用短语
    pub fn add_quick_phrase(&self, shortcut: &str, phrase: &str, desc: Option<&str>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO quick_phrases (shortcut, phrase, description) VALUES (?1, ?2, ?3)",
            params![shortcut, phrase, desc],
        )?;
        Ok(())
    }
}
