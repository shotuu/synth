use crate::database::models::{Setting, TranscriptSetting};
use crate::summary::CustomOpenAIConfig;
use sqlx::SqlitePool;

#[derive(serde::Deserialize, Debug)]
pub struct SaveModelConfigRequest {
    pub provider: String,
    pub model: String,
    #[serde(rename = "whisperModel")]
    pub whisper_model: String,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
    #[serde(rename = "ollamaEndpoint")]
    pub ollama_endpoint: Option<String>,
}

#[derive(serde::Deserialize, Debug)]
pub struct SaveTranscriptConfigRequest {
    pub provider: String,
    pub model: String,
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
}

pub struct SettingsRepository;

// Transcript providers: localWhisper, deepgram, elevenLabs, groq, openai
// Summary providers: openai, claude, ollama, groq, added openrouter
// NOTE: Handle data exclusion in the higher layer as this is database abstraction layer(using SELECT *)

impl SettingsRepository {
    pub async fn get_model_config(
        pool: &SqlitePool,
    ) -> std::result::Result<Option<Setting>, sqlx::Error> {
        let setting = sqlx::query_as::<_, Setting>("SELECT * FROM settings LIMIT 1")
            .fetch_optional(pool)
            .await?;
        Ok(setting)
    }

    pub async fn save_model_config(
        pool: &SqlitePool,
        provider: &str,
        model: &str,
        whisper_model: &str,
        ollama_endpoint: Option<&str>,
    ) -> std::result::Result<(), sqlx::Error> {
        // Using id '1' for backward compatibility
        sqlx::query(
            r#"
            INSERT INTO settings (id, provider, model, whisperModel, ollamaEndpoint)
            VALUES ('1', $1, $2, $3, $4)
            ON CONFLICT(id) DO UPDATE SET
                provider = excluded.provider,
                model = excluded.model,
                whisperModel = excluded.whisperModel,
                ollamaEndpoint = excluded.ollamaEndpoint
            "#,
        )
        .bind(provider)
        .bind(model)
        .bind(whisper_model)
        .bind(ollama_endpoint)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Namespace used for summary/LLM provider keys in the OS keychain (see
    /// database::keychain) — distinct from "transcript" since e.g. "groq" and "openai"
    /// exist as both summary providers and transcript providers with separate keys.
    const KEYCHAIN_NAMESPACE_SUMMARY: &'static str = "summary";

    pub async fn save_api_key(
        pool: &SqlitePool,
        provider: &str,
        api_key: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        // Custom OpenAI uses JSON config (customOpenAIConfig) instead of a separate API key column
        if provider == "custom-openai" {
            return Err(sqlx::Error::Protocol(
                "custom-openai provider should use save_custom_openai_config() instead of save_api_key()".into(),
            ));
        }

        let api_key_column = match provider {
            "openai" => "openaiApiKey",
            "claude" => "anthropicApiKey",
            "ollama" => "ollamaApiKey",
            "groq" => "groqApiKey",
            "openrouter" => "openRouterApiKey",
            "builtin-ai" => return Ok(()), // No API key needed
            _ => {
                return Err(sqlx::Error::Protocol(
                    format!("Invalid provider: {}", provider).into(),
                ))
            }
        };

        crate::database::keychain::save_secret(Self::KEYCHAIN_NAMESPACE_SUMMARY, provider, api_key)
            .map_err(sqlx::Error::Protocol)?;

        // Defense in depth: make sure no plaintext copy lingers in the DB row (also
        // blanks out any pre-migration legacy value from before this column existed
        // only as a migration source — see get_api_key).
        let query = format!(
            r#"
            INSERT INTO settings (id, provider, model, whisperModel, "{}")
            VALUES ('1', 'openai', 'gpt-4o-2024-11-20', 'large-v3', NULL)
            ON CONFLICT(id) DO UPDATE SET
                "{}" = NULL
            "#,
            api_key_column, api_key_column
        );
        sqlx::query(&query).execute(pool).await?;

        Ok(())
    }

    pub async fn get_api_key(
        pool: &SqlitePool,
        provider: &str,
    ) -> std::result::Result<Option<String>, sqlx::Error> {
        // Custom OpenAI uses JSON config - extract API key from there
        if provider == "custom-openai" {
            let config = Self::get_custom_openai_config(pool).await?;
            return Ok(config.and_then(|c| c.api_key));
        }

        let api_key_column = match provider {
            "openai" => "openaiApiKey",
            "ollama" => "ollamaApiKey",
            "groq" => "groqApiKey",
            "claude" => "anthropicApiKey",
            "openrouter" => "openRouterApiKey",
            "builtin-ai" => return Ok(None), // No API key needed
            _ => {
                return Err(sqlx::Error::Protocol(
                    format!("Invalid provider: {}", provider).into(),
                ))
            }
        };

        if let Some(key) = crate::database::keychain::get_secret(Self::KEYCHAIN_NAMESPACE_SUMMARY, provider)
            .map_err(sqlx::Error::Protocol)?
        {
            return Ok(Some(key));
        }

        // Nothing in the keychain yet — check for a pre-existing plaintext key left over
        // from before this migration and, if found, move it into the keychain so this
        // only ever happens once per provider.
        let query = format!(
            "SELECT {} FROM settings WHERE id = '1' LIMIT 1",
            api_key_column
        );
        let legacy_key: Option<String> = sqlx::query_scalar(&query).fetch_optional(pool).await?;
        let Some(legacy_key) = legacy_key.filter(|k| !k.is_empty()) else {
            return Ok(None);
        };

        if let Err(e) =
            crate::database::keychain::save_secret(Self::KEYCHAIN_NAMESPACE_SUMMARY, provider, &legacy_key)
        {
            log::warn!(
                "Failed to migrate legacy {} API key into system keychain, leaving it in the database for now: {}",
                provider, e
            );
            return Ok(Some(legacy_key));
        }

        let blank_query = format!(r#"UPDATE settings SET "{}" = NULL WHERE id = '1'"#, api_key_column);
        if let Err(e) = sqlx::query(&blank_query).execute(pool).await {
            log::warn!("Migrated {} API key to keychain but failed to blank the database column: {}", provider, e);
        }

        Ok(Some(legacy_key))
    }

    pub async fn get_transcript_config(
        pool: &SqlitePool,
    ) -> std::result::Result<Option<TranscriptSetting>, sqlx::Error> {
        let setting =
            sqlx::query_as::<_, TranscriptSetting>("SELECT * FROM transcript_settings LIMIT 1")
                .fetch_optional(pool)
                .await?;
        Ok(setting)

    }

    pub async fn save_transcript_config(
        pool: &SqlitePool,
        provider: &str,
        model: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO transcript_settings (id, provider, model)
            VALUES ('1', $1, $2)
            ON CONFLICT(id) DO UPDATE SET
                provider = excluded.provider,
                model = excluded.model
            "#,
        )
        .bind(provider)
        .bind(model)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Namespace for transcript-provider keys in the OS keychain — see
    /// KEYCHAIN_NAMESPACE_SUMMARY for why this needs to be distinct.
    const KEYCHAIN_NAMESPACE_TRANSCRIPT: &'static str = "transcript";

    pub async fn save_transcript_api_key(
        pool: &SqlitePool,
        provider: &str,
        api_key: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        let api_key_column = match provider {
            "localWhisper" => "whisperApiKey",
            "parakeet" => return Ok(()), // Parakeet doesn't need an API key, return early
            "deepgram" => "deepgramApiKey",
            "elevenLabs" => "elevenLabsApiKey",
            "groq" => "groqApiKey",
            "openai" => "openaiApiKey",
            _ => {
                return Err(sqlx::Error::Protocol(
                    format!("Invalid provider: {}", provider).into(),
                ))
            }
        };

        crate::database::keychain::save_secret(Self::KEYCHAIN_NAMESPACE_TRANSCRIPT, provider, api_key)
            .map_err(sqlx::Error::Protocol)?;

        // Defense in depth: don't leave a plaintext copy in the DB row.
        let query = format!(
            r#"
            INSERT INTO transcript_settings (id, provider, model, "{}")
            VALUES ('1', 'parakeet', '{}', NULL)
            ON CONFLICT(id) DO UPDATE SET
                "{}" = NULL
            "#,
            api_key_column, crate::config::DEFAULT_PARAKEET_MODEL, api_key_column
        );
        sqlx::query(&query).execute(pool).await?;

        Ok(())
    }

    pub async fn get_transcript_api_key(
        pool: &SqlitePool,
        provider: &str,
    ) -> std::result::Result<Option<String>, sqlx::Error> {
        let api_key_column = match provider {
            "localWhisper" => "whisperApiKey",
            "parakeet" => return Ok(None), // Parakeet doesn't need an API key
            "deepgram" => "deepgramApiKey",
            "elevenLabs" => "elevenLabsApiKey",
            "groq" => "groqApiKey",
            "openai" => "openaiApiKey",
            _ => {
                return Err(sqlx::Error::Protocol(
                    format!("Invalid provider: {}", provider).into(),
                ))
            }
        };

        if let Some(key) =
            crate::database::keychain::get_secret(Self::KEYCHAIN_NAMESPACE_TRANSCRIPT, provider)
                .map_err(sqlx::Error::Protocol)?
        {
            return Ok(Some(key));
        }

        // Migrate a pre-existing plaintext key, if any, exactly like get_api_key does.
        let query = format!(
            "SELECT {} FROM transcript_settings WHERE id = '1' LIMIT 1",
            api_key_column
        );
        let legacy_key: Option<String> = sqlx::query_scalar(&query).fetch_optional(pool).await?;
        let Some(legacy_key) = legacy_key.filter(|k| !k.is_empty()) else {
            return Ok(None);
        };

        if let Err(e) = crate::database::keychain::save_secret(
            Self::KEYCHAIN_NAMESPACE_TRANSCRIPT,
            provider,
            &legacy_key,
        ) {
            log::warn!(
                "Failed to migrate legacy {} transcript API key into system keychain, leaving it in the database for now: {}",
                provider, e
            );
            return Ok(Some(legacy_key));
        }

        let blank_query = format!(
            r#"UPDATE transcript_settings SET "{}" = NULL WHERE id = '1'"#,
            api_key_column
        );
        if let Err(e) = sqlx::query(&blank_query).execute(pool).await {
            log::warn!(
                "Migrated {} transcript API key to keychain but failed to blank the database column: {}",
                provider, e
            );
        }

        Ok(Some(legacy_key))
    }

    pub async fn delete_api_key(
        pool: &SqlitePool,
        provider: &str,
    ) -> std::result::Result<(), sqlx::Error> {
        // Custom OpenAI uses JSON config - clear the entire config
        if provider == "custom-openai" {
            crate::database::keychain::delete_secret(Self::KEYCHAIN_NAMESPACE_SUMMARY, Self::CUSTOM_OPENAI_PROVIDER)
                .map_err(sqlx::Error::Protocol)?;
            sqlx::query("UPDATE settings SET customOpenAIConfig = NULL WHERE id = '1'")
                .execute(pool)
                .await?;
            return Ok(());
        }

        let api_key_column = match provider {
            "openai" => "openaiApiKey",
            "ollama" => "ollamaApiKey",
            "groq" => "groqApiKey",
            "claude" => "anthropicApiKey",
            "openrouter" => "openRouterApiKey",
            "builtin-ai" => return Ok(()), // No API key needed
            _ => {
                return Err(sqlx::Error::Protocol(
                    format!("Invalid provider: {}", provider).into(),
                ))
            }
        };

        crate::database::keychain::delete_secret(Self::KEYCHAIN_NAMESPACE_SUMMARY, provider)
            .map_err(sqlx::Error::Protocol)?;

        let query = format!(
            "UPDATE settings SET {} = NULL WHERE id = '1'",
            api_key_column
        );
        sqlx::query(&query).execute(pool).await?;

        Ok(())
    }

    // ===== CUSTOM OPENAI CONFIG METHODS =====

    /// Provider name used for custom-openai's entry in the summary keychain
    /// namespace — kept as a constant since it's matched against elsewhere.
    const CUSTOM_OPENAI_PROVIDER: &'static str = "custom-openai";

    /// Gets the custom OpenAI configuration. The API key is never read from
    /// the `customOpenAIConfig` JSON column directly — it's resolved from
    /// the OS keychain (see database::keychain), with a one-time migration
    /// of any pre-existing plaintext key found in the JSON, exactly like
    /// get_api_key does for the other providers.
    ///
    /// # Returns
    /// * `Ok(Some(CustomOpenAIConfig))` - Config exists and is valid JSON
    /// * `Ok(None)` - No config stored
    /// * `Err(sqlx::Error)` - Database error
    pub async fn get_custom_openai_config(
        pool: &SqlitePool,
    ) -> std::result::Result<Option<CustomOpenAIConfig>, sqlx::Error> {
        use sqlx::Row;

        let row = sqlx::query(
            r#"
            SELECT customOpenAIConfig
            FROM settings
            WHERE id = '1'
            LIMIT 1
            "#
        )
        .fetch_optional(pool)
        .await?;

        let Some(record) = row else { return Ok(None) };
        let config_json: Option<String> = record.get("customOpenAIConfig");
        let Some(json) = config_json else { return Ok(None) };

        let mut config: CustomOpenAIConfig = serde_json::from_str(&json)
            .map_err(|e| sqlx::Error::Protocol(
                format!("Invalid JSON in customOpenAIConfig: {}", e).into()
            ))?;

        if let Some(key) = crate::database::keychain::get_secret(
            Self::KEYCHAIN_NAMESPACE_SUMMARY,
            Self::CUSTOM_OPENAI_PROVIDER,
        )
        .map_err(sqlx::Error::Protocol)?
        {
            config.api_key = Some(key);
            return Ok(Some(config));
        }

        // Nothing in the keychain yet — migrate a pre-existing plaintext key
        // found in the JSON column, if any, so this only ever happens once.
        let Some(legacy_key) = config.api_key.take().filter(|k| !k.is_empty()) else {
            return Ok(Some(config));
        };

        if let Err(e) = crate::database::keychain::save_secret(
            Self::KEYCHAIN_NAMESPACE_SUMMARY,
            Self::CUSTOM_OPENAI_PROVIDER,
            &legacy_key,
        ) {
            log::warn!(
                "Failed to migrate legacy custom-openai API key into system keychain, leaving it in the database for now: {}",
                e
            );
            config.api_key = Some(legacy_key);
            return Ok(Some(config));
        }

        // Blank the plaintext key out of the JSON column now that it lives in the keychain.
        let mut blanked = config.clone();
        blanked.api_key = None;
        if let Ok(blanked_json) = serde_json::to_string(&blanked) {
            if let Err(e) = sqlx::query("UPDATE settings SET customOpenAIConfig = $1 WHERE id = '1'")
                .bind(blanked_json)
                .execute(pool)
                .await
            {
                log::warn!("Migrated custom-openai API key to keychain but failed to blank the database column: {}", e);
            }
        }

        config.api_key = Some(legacy_key);
        Ok(Some(config))
    }

    /// Saves the custom OpenAI configuration. The API key is written to the
    /// OS keychain, never to the `customOpenAIConfig` JSON column — see
    /// get_custom_openai_config.
    ///
    /// # Arguments
    /// * `pool` - Database connection pool
    /// * `config` - CustomOpenAIConfig to save (includes endpoint, apiKey, model, maxTokens, temperature, topP)
    ///
    /// # Returns
    /// * `Ok(())` - Config saved successfully
    /// * `Err(sqlx::Error)` - Database or JSON serialization error
    pub async fn save_custom_openai_config(
        pool: &SqlitePool,
        config: &CustomOpenAIConfig,
    ) -> std::result::Result<(), sqlx::Error> {
        match config.api_key.as_ref().filter(|k| !k.is_empty()) {
            Some(key) => {
                crate::database::keychain::save_secret(
                    Self::KEYCHAIN_NAMESPACE_SUMMARY,
                    Self::CUSTOM_OPENAI_PROVIDER,
                    key,
                )
                .map_err(sqlx::Error::Protocol)?;
            }
            None => {
                crate::database::keychain::delete_secret(
                    Self::KEYCHAIN_NAMESPACE_SUMMARY,
                    Self::CUSTOM_OPENAI_PROVIDER,
                )
                .map_err(sqlx::Error::Protocol)?;
            }
        }

        // Never persist the plaintext key in the DB row.
        let mut db_config = config.clone();
        db_config.api_key = None;
        let config_json = serde_json::to_string(&db_config)
            .map_err(|e| sqlx::Error::Protocol(
                format!("Failed to serialize config to JSON: {}", e).into()
            ))?;

        // Upsert into settings table
        sqlx::query(
            r#"
            INSERT INTO settings (id, provider, model, whisperModel, customOpenAIConfig)
            VALUES ('1', 'custom-openai', $1, 'large-v3', $2)
            ON CONFLICT(id) DO UPDATE SET
                customOpenAIConfig = excluded.customOpenAIConfig
            "#,
        )
        .bind(&config.model)
        .bind(config_json)
        .execute(pool)
        .await?;

        Ok(())
    }
}
