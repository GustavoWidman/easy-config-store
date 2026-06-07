use eyre::bail;
use serde::{Serialize, de::DeserializeOwned};

mod serializer;

use std::{
    ops::{Deref, DerefMut},
    path::PathBuf,
    sync::Arc,
};

fn config_to_value<T: Serialize>(config: &T) -> Result<serializer::Value, eyre::Error> {
    let config_str = serializer::to_string(config)?;
    Ok(serializer::from_str(&config_str)?)
}

#[derive(Debug, Clone)]
pub struct ConfigStore<T: Default + Serialize + DeserializeOwned + PartialEq> {
    pub path: PathBuf,
    nest: Option<String>,
    cached: T,
}

impl<T: Default + Serialize + DeserializeOwned + PartialEq> ConfigStore<T> {
    fn preflight(path: PathBuf, nest: Option<String>) -> Result<Option<Self>, eyre::Error> {
        if path.is_dir() {
            bail!(
                "Given config path is a directory... either change the path or delete the directory."
            );
        }

        if !path.exists() {
            return Ok(Some(Self::new(path, nest)?));
        }

        if !path.is_file() {
            bail!(
                "Given config path exists and is not a file... either change the path or delete the file."
            );
        }

        Ok(None)
    }

    pub fn read(
        path: impl Into<PathBuf>,
        nest: impl Into<Option<String>>,
    ) -> Result<Self, eyre::Error> {
        let path = path.into();
        let nest = nest.into();

        if let Some(config) = Self::preflight(path.clone(), nest.clone())? {
            return Ok(config);
        }

        let config_str = std::fs::read_to_string(&path)?;
        let deserialized: serializer::Value = serializer::from_str(&config_str)?;

        let cached = match nest {
            Some(ref key) => deserialized
                .get(key)
                .ok_or_else(|| eyre::eyre!("Nested config '{}' not found", key))?
                .clone(),
            None => deserialized,
        };

        Ok(Self {
            path,
            nest,
            cached: T::deserialize(cached)?,
        })
    }

    pub fn arc(self) -> Arc<Self> {
        return Arc::new(self);
    }

    #[cfg(feature = "tokio")]
    pub async fn async_read(
        path: impl Into<PathBuf>,
        nest: impl Into<Option<String>>,
    ) -> Result<Self, eyre::Error> {
        let path = path.into();
        let nest = nest.into();

        if let Some(config) = Self::preflight(path.clone(), nest.clone())? {
            return Ok(config);
        }

        let config_str = tokio::fs::read_to_string(&path).await?;
        let deserialized: serializer::Value = serializer::from_str(&config_str)?;

        let cached = match nest {
            Some(ref key) => deserialized
                .get(key)
                .ok_or_else(|| eyre::eyre!("Nested config '{}' not found", key))?
                .clone(),
            None => deserialized,
        };

        Ok(Self {
            path,
            nest,
            cached: T::deserialize(cached)?,
        })
    }

    pub fn update(&mut self) -> eyre::Result<bool> {
        let new = Self::read(self.path.clone(), self.nest.clone())?;

        Ok(match self.cached == new.cached {
            true => false,
            false => {
                self.cached = new.cached;
                true
            }
        })
    }

    #[cfg(feature = "tokio")]
    pub async fn async_update(&mut self) -> eyre::Result<bool> {
        let new = Self::async_read(self.path.clone(), self.nest.clone()).await?;

        Ok(match self.cached == new.cached {
            true => false,
            false => {
                self.cached = new.cached;
                true
            }
        })
    }

    fn new(path: PathBuf, nest: Option<String>) -> Result<Self, eyre::Error> {
        std::fs::create_dir_all(path.parent().unwrap())?;

        let config = Self {
            path,
            nest,
            cached: T::default(),
        };

        config.save()?;

        Ok(config)
    }

    pub fn into_inner(self) -> T {
        self.cached
    }

    pub fn merge(&mut self, other_config: T) -> Result<(), eyre::Error> {
        let mut config_value = config_to_value(&self.cached)?;
        let other_value = config_to_value(&other_config)?;
        serializer::merge_values(&mut config_value, other_value)?;
        self.cached = T::deserialize(config_value)?;
        Ok(())
    }

    pub fn overwrite(&mut self, other_config: T) -> Result<(), eyre::Error> {
        let mut config_value = config_to_value(&self.cached)?;
        let other_value = config_to_value(&other_config)?;
        serializer::overwrite_values(&mut config_value, other_value);
        self.cached = T::deserialize(config_value)?;
        Ok(())
    }

    pub fn save(&self) -> Result<(), eyre::Error> {
        let to_write = match &self.nest {
            Some(key) => {
                // Read existing config or create empty map
                let mut root: std::collections::HashMap<String, serializer::Value> =
                    if self.path.exists() {
                        let content = std::fs::read_to_string(&self.path)?;
                        serializer::from_str(&content)?
                    } else {
                        std::collections::HashMap::new()
                    };

                // Serialize cached to string, then parse to Value
                let cached_str = serializer::to_string(&self.cached)?;
                let cached_value: serializer::Value = serializer::from_str(&cached_str)?;

                root.insert(key.clone(), cached_value);
                serializer::to_string(&root)?
            }
            None => serializer::to_string(&self.cached)?,
        };

        std::fs::write(&self.path, to_write)?;
        Ok(())
    }

    #[cfg(feature = "tokio")]
    pub async fn async_save(&self) -> Result<(), eyre::Error> {
        let to_write = match &self.nest {
            Some(key) => {
                // Read existing config or create empty map
                let mut root: std::collections::HashMap<String, serializer::Value> =
                    if self.path.exists() {
                        let content = std::fs::read_to_string(&self.path)?;
                        serializer::from_str(&content)?
                    } else {
                        std::collections::HashMap::new()
                    };

                // Serialize cached to string, then parse to Value
                let cached_str = serializer::to_string(&self.cached)?;
                let cached_value: serializer::Value = serializer::from_str(&cached_str)?;

                root.insert(key.clone(), cached_value);
                serializer::to_string(&root)?
            }
            None => serializer::to_string(&self.cached)?,
        };

        tokio::fs::write(&self.path, to_write).await?;
        Ok(())
    }
}

impl<T: Default + Serialize + DeserializeOwned + PartialEq> Deref for ConfigStore<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.cached
    }
}

impl<T: Default + Serialize + DeserializeOwned + PartialEq> DerefMut for ConfigStore<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.cached
    }
}

impl<T: Default + Serialize + DeserializeOwned + PartialEq> PartialEq for ConfigStore<T> {
    fn eq(&self, other: &Self) -> bool {
        self.cached == other.cached
    }
}
impl<T: Default + Serialize + DeserializeOwned + PartialEq> Eq for ConfigStore<T> {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    struct TestConfig {
        name: Option<String>,
        database: DatabaseConfig,
        features: FeatureConfig,
    }

    #[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    struct DatabaseConfig {
        url: Option<String>,
        pool_size: Option<u16>,
    }

    #[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    struct FeatureConfig {
        enabled: Option<bool>,
    }

    fn store(config: TestConfig) -> ConfigStore<TestConfig> {
        ConfigStore {
            path: PathBuf::from("/tmp/easy-config-store-test.toml"),
            nest: None,
            cached: config,
        }
    }

    #[test]
    fn merge_adds_missing_nested_values() {
        let mut config = store(TestConfig {
            name: Some("app".to_owned()),
            database: DatabaseConfig {
                url: Some("sqlite://app.db".to_owned()),
                pool_size: None,
            },
            features: FeatureConfig::default(),
        });
        let other = TestConfig {
            name: None,
            database: DatabaseConfig {
                url: None,
                pool_size: Some(8),
            },
            features: FeatureConfig {
                enabled: Some(true),
            },
        };

        config.merge(other).expect("merge succeeds");

        assert_eq!(config.name.as_deref(), Some("app"));
        assert_eq!(config.database.url.as_deref(), Some("sqlite://app.db"));
        assert_eq!(config.database.pool_size, Some(8));
        assert_eq!(config.features.enabled, Some(true));
    }

    #[test]
    fn merge_allows_equal_values() {
        let mut config = store(TestConfig {
            name: Some("app".to_owned()),
            ..Default::default()
        });
        let other = TestConfig {
            name: Some("app".to_owned()),
            ..Default::default()
        };

        config.merge(other).expect("equal values do not conflict");

        assert_eq!(config.name.as_deref(), Some("app"));
    }

    #[test]
    fn merge_errors_on_conflicting_values() {
        let mut config = store(TestConfig {
            name: Some("app".to_owned()),
            ..Default::default()
        });
        let other = TestConfig {
            name: Some("other".to_owned()),
            ..Default::default()
        };

        let error = config.merge(other).expect_err("conflict should fail");

        assert!(error.to_string().contains("conflicting config value"));
        assert_eq!(config.name.as_deref(), Some("app"));
    }

    #[test]
    fn overwrite_replaces_conflicting_values_and_keeps_missing_values() {
        let mut config = store(TestConfig {
            name: Some("app".to_owned()),
            database: DatabaseConfig {
                url: Some("sqlite://app.db".to_owned()),
                pool_size: Some(4),
            },
            features: FeatureConfig::default(),
        });
        let other = TestConfig {
            name: Some("other".to_owned()),
            database: DatabaseConfig {
                url: None,
                pool_size: Some(8),
            },
            features: FeatureConfig {
                enabled: Some(true),
            },
        };

        config.overwrite(other).expect("overwrite succeeds");

        assert_eq!(config.name.as_deref(), Some("other"));
        assert_eq!(config.database.url.as_deref(), Some("sqlite://app.db"));
        assert_eq!(config.database.pool_size, Some(8));
        assert_eq!(config.features.enabled, Some(true));
    }
}
