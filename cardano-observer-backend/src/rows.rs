//! Column extraction helpers for dynamic SQL rows. Decode failures are logged
//! and surface as JSON null rather than a 500, keeping responses robust across
//! minor database schema drift.

use serde_json::Value;
use sqlx::postgres::PgRow;
use sqlx::Row;

pub trait RowExt {
    fn s(&self, col: &str) -> Option<String>;
    fn int4(&self, col: &str) -> Option<i32>;
    fn int8(&self, col: &str) -> Option<i64>;
    fn float8(&self, col: &str) -> Option<f64>;
    fn boolean(&self, col: &str) -> Option<bool>;
    fn json(&self, col: &str) -> Option<Value>;
}

fn get<T>(row: &PgRow, col: &str) -> Option<T>
where
    for<'r> T: sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
{
    match row.try_get::<Option<T>, _>(col) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("column {col}: decode failed: {e}");
            None
        }
    }
}

impl RowExt for PgRow {
    fn s(&self, col: &str) -> Option<String> {
        get(self, col)
    }

    fn int4(&self, col: &str) -> Option<i32> {
        get(self, col)
    }

    fn int8(&self, col: &str) -> Option<i64> {
        get(self, col)
    }

    fn float8(&self, col: &str) -> Option<f64> {
        get(self, col)
    }

    fn boolean(&self, col: &str) -> Option<bool> {
        get(self, col)
    }

    fn json(&self, col: &str) -> Option<Value> {
        get(self, col)
    }
}
