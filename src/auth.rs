use anyhow::{Context, Result};
use tokio_postgres::{Client, NoTls};

use crate::metrics;

/// Configuration for authentication
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Name of the PostgreSQL function to call for authentication
    /// The function should accept a token parameter and return a boolean or user info
    pub pg_function: String,
}

impl AuthConfig {
    /// Create a new auth configuration with the specified PostgreSQL function
    pub fn new(pg_function: impl Into<String>) -> Self {
        Self {
            pg_function: pg_function.into(),
        }
    }
}

/// Authentication result containing user information
#[derive(Debug, Clone)]
pub struct AuthResult {
    /// User ID or identifier returned from the auth function
    pub user_id: String,
    /// Whether authentication was successful
    pub authenticated: bool,
}

/// Authenticator that verifies tokens against PostgreSQL
pub struct Authenticator {
    client: Client,
    config: AuthConfig,
}

impl Authenticator {
    /// Create a new authenticator by connecting to PostgreSQL with the given connection string and config
    pub async fn new(db_url: &str, config: AuthConfig) -> Result<Self> {
        let (client, connection) = tokio_postgres::connect(db_url, NoTls)
            .await
            .context("Failed to connect to PostgreSQL")
            .inspect_err(|_| metrics::errors::postgres_connection())?;

        // Spawn the connection to run in the background
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::error!("PostgreSQL connection error: {}", e);
            }
        });

        Ok(Self { client, config })
    }

    /// Authenticate a user with the provided token
    ///
    /// Calls the configured PostgreSQL function with the token and returns the result.
    /// The PG function should return a row with (user_id TEXT, authenticated BOOLEAN)
    /// or just a BOOLEAN for simple yes/no authentication.
    pub async fn authenticate(&self, token: &str) -> Result<AuthResult> {
        let query = format!("SELECT * FROM {}($1)", self.config.pg_function);

        tracing::debug!(
            "Authenticating with function '{}' and token",
            self.config.pg_function
        );

        let rows = self.client.query(&query, &[&token]).await.context(format!(
            "Failed to call authentication function '{}'",
            self.config.pg_function
        ))?;

        if rows.is_empty() {
            return Ok(AuthResult {
                user_id: String::new(),
                authenticated: false,
            });
        }

        let row = &rows[0];

        // Try to parse result as (user_id, authenticated) or just authenticated
        let result = if row.len() >= 2 {
            // Function returns (user_id, authenticated)
            let user_id: String = row
                .try_get(0)
                .context("Failed to get user_id from auth result")?;
            let authenticated: bool = row
                .try_get(1)
                .context("Failed to get authenticated from auth result")?;
            AuthResult {
                user_id,
                authenticated,
            }
        } else if row.len() == 1 {
            // Function returns just boolean
            let authenticated: bool = row
                .try_get(0)
                .context("Failed to get authenticated from auth result")?;
            AuthResult {
                user_id: String::new(),
                authenticated,
            }
        } else {
            return Ok(AuthResult {
                user_id: String::new(),
                authenticated: false,
            });
        };

        tracing::debug!(
            "Authentication result: user_id='{}', authenticated={}",
            result.user_id,
            result.authenticated
        );

        Ok(result)
    }

    /// Simple authentication check that only returns success/failure
    pub async fn verify_token(&self, token: &str) -> Result<bool> {
        let result = self.authenticate(token).await?;
        Ok(result.authenticated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Example PostgreSQL function for testing:
    ///
    /// CREATE OR REPLACE FUNCTION authenticate_user(token TEXT)
    /// RETURNS TABLE(user_id TEXT, authenticated BOOLEAN) AS $$
    /// BEGIN
    ///   IF token = 'valid_token' THEN
    ///     RETURN QUERY SELECT 'user_123'::TEXT, TRUE;
    ///   ELSE
    ///     RETURN QUERY SELECT ''::TEXT, FALSE;
    ///   END IF;
    /// END;
    /// $$ LANGUAGE plpgsql;
    ///
    /// Or simpler boolean-only version:
    ///
    /// CREATE OR REPLACE FUNCTION verify_token(token TEXT)
    /// RETURNS BOOLEAN AS $$
    /// BEGIN
    ///   RETURN token = 'valid_token';
    /// END;
    /// $$ LANGUAGE plpgsql;

    #[tokio::test]
    #[ignore] // Requires PostgreSQL with auth function set up
    async fn test_authenticate_with_valid_token() {
        let db_url = std::env::var("TEST_DATABASE_URL").unwrap();
        let config = AuthConfig::new("authenticate_user");
        let authenticator = Authenticator::new(&db_url, config).await.unwrap();

        let result = authenticator.authenticate("valid_token").await.unwrap();
        assert!(result.authenticated);
        assert_eq!(result.user_id, "user_123");
    }

    #[tokio::test]
    #[ignore] // Requires PostgreSQL with auth function set up
    async fn test_verify_token() {
        let db_url = std::env::var("TEST_DATABASE_URL").unwrap();
        let config = AuthConfig::new("verify_token");
        let authenticator = Authenticator::new(&db_url, config).await.unwrap();

        let is_valid = authenticator.verify_token("valid_token").await.unwrap();
        assert!(is_valid);

        let is_invalid = authenticator.verify_token("invalid_token").await.unwrap();
        assert!(!is_invalid);
    }
}
