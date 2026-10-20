//! Postgres relay (NeonDB by default; any Postgres-compatible URL works). Everything stored here is
//! ciphertext plus ids, kinds and clocks. TLS uses rustls with the bundled Mozilla roots.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio_postgres::config::SslMode;
use tokio_postgres::{Client, Config, NoTls};

use super::backend::{CommandRow, DeviceRow, RemoteOp, SyncBackend};
use crate::crypto::{Account, Sealed};
use crate::error::{CoreError, Result};
use crate::util::now_ms;

const SCHEMA: &str = "
CREATE SCHEMA IF NOT EXISTS dolphinpod;

CREATE TABLE IF NOT EXISTS dolphinpod.account (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    generation INTEGER NOT NULL,
    body       TEXT NOT NULL
);

-- The operation log. 'id' is a keyed hash of the real record id; 'ciphertext' holds the sealed field patch.
CREATE TABLE IF NOT EXISTS dolphinpod.records (
    seq        BIGSERIAL PRIMARY KEY,
    id         TEXT NOT NULL,
    kind       TEXT NOT NULL,
    hlc        TEXT NOT NULL,
    device_id  TEXT NOT NULL,
    ciphertext BYTEA NOT NULL,
    nonce      BYTEA NOT NULL,
    deleted    BOOLEAN NOT NULL DEFAULT FALSE,
    UNIQUE (kind, id, hlc, device_id)
);

CREATE TABLE IF NOT EXISTS dolphinpod.devices (
    id         TEXT PRIMARY KEY,
    last_seen  BIGINT NOT NULL,
    revoked    BOOLEAN NOT NULL DEFAULT FALSE,
    ciphertext BYTEA NOT NULL,
    nonce      BYTEA NOT NULL
);

CREATE TABLE IF NOT EXISTS dolphinpod.commands (
    seq         BIGSERIAL PRIMARY KEY,
    id          TEXT NOT NULL UNIQUE,
    from_device TEXT NOT NULL,
    target      TEXT NOT NULL,
    created_at  BIGINT NOT NULL,
    expires_at  BIGINT NOT NULL,
    ciphertext  BYTEA NOT NULL,
    nonce       BYTEA NOT NULL,
    status      TEXT NOT NULL DEFAULT 'new'
);
CREATE INDEX IF NOT EXISTS commands_target ON dolphinpod.commands (target, status);

CREATE TABLE IF NOT EXISTS dolphinpod.blobs (
    blob_id    TEXT NOT NULL,
    idx        INTEGER NOT NULL,
    ciphertext BYTEA NOT NULL,
    nonce      BYTEA NOT NULL,
    PRIMARY KEY (blob_id, idx)
);
";

pub struct PostgresBackend {
    config: Config,
    client: Mutex<Option<Arc<Client>>>,
}

fn pg_err(e: tokio_postgres::Error) -> CoreError {
    let detail = e.as_db_error().map(|d| d.message().to_string()).unwrap_or_else(|| e.to_string());
    CoreError::Provider(format!("sync database: {detail}"))
}

impl PostgresBackend {
    /// `url` is a `postgres://` or `postgresql://` connection string.
    pub fn new(url: &str) -> Result<Self> {
        let config: Config = url.trim().parse().map_err(|e: tokio_postgres::Error| {
            CoreError::invalid(format!("that is not a valid Postgres connection URL ({e})"))
        })?;
        if config.get_hosts().is_empty() {
            return Err(CoreError::invalid("the connection URL has no host"));
        }
        Ok(Self { config, client: Mutex::new(None) })
    }

    async fn connect(&self) -> Result<Client> {
        let mut config = self.config.clone();
        config.connect_timeout(Duration::from_secs(20));
        if config.get_ssl_mode() == SslMode::Disable {
            let (client, conn) = config.connect(NoTls).await.map_err(pg_err)?;
            tokio::spawn(async move {
                let _ = conn.await;
            });
            return Ok(client);
        }
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|e| CoreError::provider(e.to_string()))?
            .with_root_certificates(roots)
            .with_no_client_auth();
        let (client, conn) = config.connect(tokio_postgres_rustls::MakeRustlsConnect::new(tls)).await.map_err(pg_err)?;
        tokio::spawn(async move {
            let _ = conn.await;
        });
        Ok(client)
    }

    /// A live client, reconnecting if the last one died (Neon suspends idle compute).
    async fn client(&self) -> Result<Arc<Client>> {
        let mut slot = self.client.lock().await;
        if let Some(c) = slot.as_ref()
            && !c.is_closed()
        {
            return Ok(c.clone());
        }
        let c = Arc::new(self.connect().await?);
        *slot = Some(c.clone());
        Ok(c)
    }
}

#[cfg(test)]
impl PostgresBackend {
    pub async fn client_for_tests(&self) -> Arc<Client> {
        self.client().await.unwrap()
    }
}

fn sealed_of(nonce: Vec<u8>, ciphertext: Vec<u8>) -> Sealed {
    Sealed { nonce, ciphertext }
}

#[async_trait]
impl SyncBackend for PostgresBackend {
    async fn init(&self) -> Result<()> {
        self.client().await?.batch_execute(SCHEMA).await.map_err(pg_err)
    }

    async fn get_account(&self) -> Result<Option<Account>> {
        let row = self.client().await?.query_opt("SELECT body FROM dolphinpod.account WHERE id = 1", &[]).await.map_err(pg_err)?;
        row.map(|r| serde_json::from_str(&r.get::<_, String>(0)).map_err(Into::into)).transpose()
    }

    async fn create_account(&self, account: &Account) -> Result<bool> {
        let body = serde_json::to_string(account)?;
        let n = self
            .client()
            .await?
            .execute(
                "INSERT INTO dolphinpod.account (id, generation, body) VALUES (1, $1, $2) ON CONFLICT (id) DO NOTHING",
                &[&(account.generation as i32), &body],
            )
            .await
            .map_err(pg_err)?;
        Ok(n == 1)
    }

    async fn replace_account(&self, account: &Account, expected_generation: u32) -> Result<bool> {
        let body = serde_json::to_string(account)?;
        let n = self
            .client()
            .await?
            .execute(
                "UPDATE dolphinpod.account SET generation = $1, body = $2 WHERE id = 1 AND generation = $3",
                &[&(account.generation as i32), &body, &(expected_generation as i32)],
            )
            .await
            .map_err(pg_err)?;
        Ok(n == 1)
    }

    async fn push_ops(&self, author: &str, ops: &[RemoteOp]) -> Result<()> {
        if ops.is_empty() {
            return Ok(());
        }
        let client = self.client().await?;
        let revoked = client
            .query_opt("SELECT revoked FROM dolphinpod.devices WHERE id = $1", &[&author])
            .await
            .map_err(pg_err)?
            .is_some_and(|r| r.get::<_, bool>(0));
        if revoked {
            return Err(CoreError::Permission("this device was removed from the pod".into()));
        }
        // One writer at a time, so sequence numbers become visible in order and a reader's cursor can never skip an operation.
        client.batch_execute("BEGIN").await.map_err(pg_err)?;
        let result: Result<()> = async {
            client.execute("SELECT pg_advisory_xact_lock(7467001)", &[]).await.map_err(pg_err)?;
            for op in ops {
                client
                    .execute(
                        "INSERT INTO dolphinpod.records (id, kind, hlc, device_id, ciphertext, nonce, deleted)
                         VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (kind, id, hlc, device_id) DO NOTHING",
                        &[&op.id, &op.kind, &op.hlc, &op.device_id, &op.sealed.ciphertext, &op.sealed.nonce, &op.deleted],
                    )
                    .await
                    .map_err(pg_err)?;
            }
            Ok(())
        }
        .await;
        match result {
            Ok(()) => client.batch_execute("COMMIT").await.map_err(pg_err),
            Err(e) => {
                let _ = client.batch_execute("ROLLBACK").await;
                Err(e)
            }
        }
    }

    async fn pull_ops(&self, after: i64, limit: usize) -> Result<Vec<RemoteOp>> {
        let rows = self
            .client()
            .await?
            .query(
                "SELECT seq, id, kind, hlc, device_id, ciphertext, nonce, deleted FROM dolphinpod.records
                 WHERE seq > $1 ORDER BY seq LIMIT $2",
                &[&after, &(limit as i64)],
            )
            .await
            .map_err(pg_err)?;
        Ok(rows
            .into_iter()
            .map(|r| RemoteOp {
                seq: r.get(0),
                id: r.get(1),
                kind: r.get(2),
                hlc: r.get(3),
                device_id: r.get(4),
                sealed: sealed_of(r.get(6), r.get(5)),
                deleted: r.get(7),
            })
            .collect())
    }

    async fn upsert_device(&self, d: &DeviceRow) -> Result<()> {
        self.client()
            .await?
            .execute(
                "INSERT INTO dolphinpod.devices (id, last_seen, revoked, ciphertext, nonce) VALUES ($1, $2, FALSE, $3, $4)
                 ON CONFLICT (id) DO UPDATE SET last_seen = excluded.last_seen, ciphertext = excluded.ciphertext, nonce = excluded.nonce",
                &[&d.id, &d.last_seen_ms, &d.sealed.ciphertext, &d.sealed.nonce],
            )
            .await
            .map_err(pg_err)?;
        Ok(())
    }

    async fn list_devices(&self) -> Result<Vec<DeviceRow>> {
        let rows = self
            .client()
            .await?
            .query("SELECT id, last_seen, revoked, ciphertext, nonce FROM dolphinpod.devices ORDER BY id", &[])
            .await
            .map_err(pg_err)?;
        Ok(rows
            .into_iter()
            .map(|r| DeviceRow { id: r.get(0), last_seen_ms: r.get(1), revoked: r.get(2), sealed: sealed_of(r.get(4), r.get(3)) })
            .collect())
    }

    async fn revoke_device(&self, id: &str) -> Result<()> {
        self.client().await?.execute("UPDATE dolphinpod.devices SET revoked = TRUE WHERE id = $1", &[&id]).await.map_err(pg_err)?;
        Ok(())
    }

    async fn put_command(&self, c: &CommandRow) -> Result<()> {
        self.client()
            .await?
            .execute(
                "INSERT INTO dolphinpod.commands (id, from_device, target, created_at, expires_at, ciphertext, nonce, status)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, 'new') ON CONFLICT (id) DO NOTHING",
                &[&c.id, &c.from_device_id, &c.target_device_id, &c.created_at, &c.expires_at, &c.sealed.ciphertext, &c.sealed.nonce],
            )
            .await
            .map_err(pg_err)?;
        Ok(())
    }

    async fn pull_commands(&self, target: &str, limit: usize) -> Result<Vec<CommandRow>> {
        let rows = self
            .client()
            .await?
            .query(
                "SELECT seq, id, from_device, target, created_at, expires_at, ciphertext, nonce, status FROM dolphinpod.commands
                 WHERE target = $1 AND status = 'new' AND expires_at > $2 ORDER BY seq LIMIT $3",
                &[&target, &now_ms(), &(limit as i64)],
            )
            .await
            .map_err(pg_err)?;
        Ok(rows.into_iter().map(command_of).collect())
    }

    async fn get_command(&self, id: &str) -> Result<Option<CommandRow>> {
        let row = self
            .client()
            .await?
            .query_opt(
                "SELECT seq, id, from_device, target, created_at, expires_at, ciphertext, nonce, status FROM dolphinpod.commands WHERE id = $1",
                &[&id],
            )
            .await
            .map_err(pg_err)?;
        Ok(row.map(command_of))
    }

    async fn set_command_status(&self, id: &str, status: &str) -> Result<()> {
        self.client().await?.execute("UPDATE dolphinpod.commands SET status = $2 WHERE id = $1", &[&id, &status]).await.map_err(pg_err)?;
        Ok(())
    }

    async fn put_blob(&self, id: &str, chunks: &[Sealed]) -> Result<()> {
        let client = self.client().await?;
        client.batch_execute("BEGIN").await.map_err(pg_err)?;
        let result: Result<()> = async {
            client.execute("DELETE FROM dolphinpod.blobs WHERE blob_id = $1", &[&id]).await.map_err(pg_err)?;
            for (i, c) in chunks.iter().enumerate() {
                client
                    .execute(
                        "INSERT INTO dolphinpod.blobs (blob_id, idx, ciphertext, nonce) VALUES ($1, $2, $3, $4)",
                        &[&id, &(i as i32), &c.ciphertext, &c.nonce],
                    )
                    .await
                    .map_err(pg_err)?;
            }
            Ok(())
        }
        .await;
        match result {
            Ok(()) => client.batch_execute("COMMIT").await.map_err(pg_err),
            Err(e) => {
                let _ = client.batch_execute("ROLLBACK").await;
                Err(e)
            }
        }
    }

    async fn get_blob(&self, id: &str) -> Result<Option<Vec<Sealed>>> {
        let rows = self
            .client()
            .await?
            .query("SELECT ciphertext, nonce FROM dolphinpod.blobs WHERE blob_id = $1 ORDER BY idx", &[&id])
            .await
            .map_err(pg_err)?;
        Ok((!rows.is_empty()).then(|| rows.into_iter().map(|r| sealed_of(r.get(1), r.get(0))).collect()))
    }

    async fn has_blob(&self, id: &str) -> Result<bool> {
        Ok(self.client().await?.query_opt("SELECT 1 FROM dolphinpod.blobs WHERE blob_id = $1 LIMIT 1", &[&id]).await.map_err(pg_err)?.is_some())
    }
}

fn command_of(r: tokio_postgres::Row) -> CommandRow {
    CommandRow {
        seq: r.get(0),
        id: r.get(1),
        from_device_id: r.get(2),
        target_device_id: r.get(3),
        created_at: r.get(4),
        expires_at: r.get(5),
        sealed: sealed_of(r.get(7), r.get(6)),
        status: r.get(8),
    }
}
