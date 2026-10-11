use std::path::PathBuf;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StoredFile {
    pub id: String,
    pub original_name: String,
    pub stored_name: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub sha256_hex: String,
    pub uploaded_at: String,
    pub uploaded_by: String,
}

#[derive(Clone)]
pub struct Database {
    path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct NewFile {
    pub file_id: String,
    pub original_name: String,
    pub stored_name: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub sha256_hex: String,
    pub uploaded_by: String,
}

impl Database {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn connect(&self) -> Result<Connection> {
        let conn = Connection::open(&self.path)
            .with_context(|| format!("failed to open database at {}", self.path.display()))?;
        conn.execute("PRAGMA foreign_keys = ON", [])?;
        Ok(conn)
    }

    pub fn init(&self) -> Result<()> {
        let conn = self.connect()?;
        conn.execute_batch(
            r#"
                CREATE TABLE IF NOT EXISTS users (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    username TEXT NOT NULL UNIQUE,
                    password_hash TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );
                CREATE TABLE IF NOT EXISTS files (
                    id TEXT PRIMARY KEY,
                    original_name TEXT NOT NULL,
                    stored_name TEXT NOT NULL,
                    content_type TEXT NOT NULL,
                    size_bytes INTEGER NOT NULL,
                    sha256_hex TEXT NOT NULL,
                    uploaded_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    uploaded_by TEXT NOT NULL
                );
            "#,
        )?;
        Ok(())
    }

    pub fn count_users(&self) -> Result<i64> {
        let conn = self.connect()?;
        let count = conn.query_row("SELECT COUNT(*) FROM users", [], |row| row.get::<_, i64>(0))?;
        Ok(count)
    }

    pub fn upsert_user(&self, username: &str, password_hash: &str) -> Result<()> {
        let conn = self.connect()?;
        conn.execute(
            r#"
                INSERT INTO users (username, password_hash)
                VALUES (?1, ?2)
                ON CONFLICT(username) DO UPDATE SET password_hash=excluded.password_hash
            "#,
            params![username, password_hash],
        )?;
        Ok(())
    }

    pub fn get_user_by_username(&self, username: &str) -> Result<Option<User>> {
        let conn = self.connect()?;
        let user = conn
            .query_row(
                "SELECT id, username, password_hash, created_at FROM users WHERE username = ?1",
                params![username],
                |row| {
                    Ok(User {
                        id: row.get(0)?,
                        username: row.get(1)?,
                        password_hash: row.get(2)?,
                        created_at: row.get(3)?,
                    })
                },
            )
            .optional()?;
        Ok(user)
    }

    pub fn insert_file(&self, file: &NewFile) -> Result<()> {
        let conn = self.connect()?;
        conn.execute(
            r#"
                INSERT INTO files (id, original_name, stored_name, content_type, size_bytes, sha256_hex, uploaded_by)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                file.file_id.as_str(),
                file.original_name.as_str(),
                file.stored_name.as_str(),
                file.content_type.as_str(),
                file.size_bytes,
                file.sha256_hex.as_str(),
                file.uploaded_by.as_str(),
            ],
        )?;
        Ok(())
    }

    pub fn list_files(&self) -> Result<Vec<StoredFile>> {
        let conn = self.connect()?;
        let mut statement = conn.prepare(
            r#"
                SELECT id, original_name, stored_name, content_type, size_bytes, sha256_hex, uploaded_at, uploaded_by
                FROM files
                ORDER BY uploaded_at DESC
            "#,
        )?;
        let rows = statement.query_map([], |row| {
            Ok(StoredFile {
                id: row.get(0)?,
                original_name: row.get(1)?,
                stored_name: row.get(2)?,
                content_type: row.get(3)?,
                size_bytes: row.get(4)?,
                sha256_hex: row.get(5)?,
                uploaded_at: row.get(6)?,
                uploaded_by: row.get(7)?,
            })
        })?;

        let mut files = Vec::new();
        for row in rows {
            files.push(row?);
        }
        Ok(files)
    }

    pub fn get_file(&self, file_id: &str) -> Result<Option<StoredFile>> {
        let conn = self.connect()?;
        let file = conn
            .query_row(
                r#"
                    SELECT id, original_name, stored_name, content_type, size_bytes, sha256_hex, uploaded_at, uploaded_by
                    FROM files
                    WHERE id = ?1
                "#,
                params![file_id],
                |row| {
                    Ok(StoredFile {
                        id: row.get(0)?,
                        original_name: row.get(1)?,
                        stored_name: row.get(2)?,
                        content_type: row.get(3)?,
                        size_bytes: row.get(4)?,
                        sha256_hex: row.get(5)?,
                        uploaded_at: row.get(6)?,
                        uploaded_by: row.get(7)?,
                    })
                },
            )
            .optional()?;
        Ok(file)
    }

    pub fn delete_file(&self, file_id: &str) -> Result<()> {
        let conn = self.connect()?;
        conn.execute("DELETE FROM files WHERE id = ?1", params![file_id])?;
        Ok(())
    }
}
