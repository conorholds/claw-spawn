use async_trait::async_trait;
use claw_spawn::domain::{Droplet, DropletStatus};
use claw_spawn::infrastructure::{DropletRepository, RepositoryError};
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub struct PostgresDropletRepository {
    pool: PgPool,
}

impl PostgresDropletRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl DropletRepository for PostgresDropletRepository {
    async fn create(&self, droplet: &Droplet) -> Result<(), RepositoryError> {
        let status_str = droplet_status_to_string(&droplet.status);

        sqlx::query(
            r#"
            INSERT INTO droplets (id, name, region, size, image, status, ip_address, bot_id, created_at, destroyed_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
        )
        .bind(droplet.id)
        .bind(&droplet.name)
        .bind(&droplet.region)
        .bind(&droplet.size)
        .bind(&droplet.image)
        .bind(status_str)
        .bind(&droplet.ip_address)
        .bind(droplet.bot_id)
        .bind(droplet.created_at)
        .bind(droplet.destroyed_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_by_id(&self, id: i64) -> Result<Droplet, RepositoryError> {
        let row = sqlx::query(
            r#"
            SELECT id, name, region, size, image, status, ip_address, bot_id, created_at, destroyed_at
            FROM droplets
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => RepositoryError::NotFound(format!("Droplet {}", id)),
            _ => RepositoryError::DatabaseError(e),
        })?;

        Ok(row_to_droplet(&row)?)
    }

    async fn update_bot_assignment(
        &self,
        droplet_id: i64,
        bot_id: Option<Uuid>,
    ) -> Result<(), RepositoryError> {
        sqlx::query(
            r#"
            UPDATE droplets
            SET bot_id = $1
            WHERE id = $2
            "#,
        )
        .bind(bot_id)
        .bind(droplet_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn update_status(&self, droplet_id: i64, status: &str) -> Result<(), RepositoryError> {
        sqlx::query(
            r#"
            UPDATE droplets
            SET status = $1
            WHERE id = $2
            "#,
        )
        .bind(status)
        .bind(droplet_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn update_ip(&self, droplet_id: i64, ip: Option<String>) -> Result<(), RepositoryError> {
        sqlx::query(
            r#"
            UPDATE droplets
            SET ip_address = $1
            WHERE id = $2
            "#,
        )
        .bind(ip)
        .bind(droplet_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn mark_destroyed(&self, droplet_id: i64) -> Result<(), RepositoryError> {
        sqlx::query(
            r#"
            UPDATE droplets
            SET status = 'destroyed', destroyed_at = $1
            WHERE id = $2
            "#,
        )
        .bind(chrono::Utc::now())
        .bind(droplet_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

fn droplet_status_to_string(status: &DropletStatus) -> String {
    match status {
        DropletStatus::New => "new".to_string(),
        DropletStatus::Active => "active".to_string(),
        DropletStatus::Off => "off".to_string(),
        DropletStatus::Destroyed => "destroyed".to_string(),
        DropletStatus::Error => "error".to_string(),
    }
}

fn string_to_droplet_status(status: &str) -> Result<DropletStatus, RepositoryError> {
    match status {
        "new" => Ok(DropletStatus::New),
        "active" => Ok(DropletStatus::Active),
        "off" => Ok(DropletStatus::Off),
        "destroyed" => Ok(DropletStatus::Destroyed),
        "error" => Ok(DropletStatus::Error),
        _ => Err(RepositoryError::InvalidData(format!(
            "Unknown droplet status: {}",
            status
        ))),
    }
}

fn row_to_droplet(row: &sqlx::postgres::PgRow) -> Result<Droplet, RepositoryError> {
    let status_str: String = row.try_get("status")?;

    Ok(Droplet {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        region: row.try_get("region")?,
        size: row.try_get("size")?,
        image: row.try_get("image")?,
        status: string_to_droplet_status(&status_str)?,
        ip_address: row.try_get("ip_address")?,
        bot_id: row.try_get("bot_id")?,
        created_at: row.try_get("created_at")?,
        destroyed_at: row.try_get("destroyed_at")?,
    })
}
