/*
 * SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
 * SPDX-License-Identifier: Apache-2.0
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 * http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use std::sync::Arc;

use async_trait::async_trait;

use crate::credentials::{CredentialKey, CredentialWriter, Credentials};
use crate::SecretsError;

/// Dispatches credential writes and deletes to different backends by path
/// prefix. Routes are evaluated longest-prefix-first; `default_writer` handles
/// paths that match no specific prefix (including the normalized catch-all).
pub struct RoutedCredentialWriter {
    routes: Vec<(String, Arc<dyn CredentialWriter>)>,
    default_writer: Arc<dyn CredentialWriter>,
}

impl RoutedCredentialWriter {
    /// Build a routed writer from `(prefix, writer)` entries. Prefixes use the
    /// same normalization as api-core `BackendRouting`: the catch-all `"/"`
    /// becomes the empty prefix, and other prefixes gain a trailing slash so
    /// they match whole path segments.
    pub fn new(
        routes: Vec<(String, Arc<dyn CredentialWriter>)>,
        default_writer: Arc<dyn CredentialWriter>,
    ) -> Self {
        let mut routes: Vec<(String, Arc<dyn CredentialWriter>)> = routes
            .into_iter()
            .map(|(prefix, writer)| (normalize_prefix(&prefix), writer))
            .collect();
        routes.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
        Self {
            routes,
            default_writer,
        }
    }

    fn writer_for_path(&self, path: &str) -> &Arc<dyn CredentialWriter> {
        self.routes
            .iter()
            .find(|(prefix, _)| path.starts_with(prefix.as_str()))
            .map(|(_, writer)| writer)
            .unwrap_or(&self.default_writer)
    }
}

const CATCH_ALL: &str = "/";

fn normalize_prefix(prefix: &str) -> String {
    if prefix == CATCH_ALL {
        String::new()
    } else if prefix.ends_with('/') {
        prefix.to_string()
    } else {
        format!("{prefix}/")
    }
}

#[async_trait]
impl CredentialWriter for RoutedCredentialWriter {
    async fn set_credentials(
        &self,
        key: &CredentialKey,
        credentials: &Credentials,
    ) -> Result<(), SecretsError> {
        let path = key.to_key_str();
        self.writer_for_path(path.as_ref())
            .set_credentials(key, credentials)
            .await
    }

    async fn create_credentials(
        &self,
        key: &CredentialKey,
        credentials: &Credentials,
    ) -> Result<(), SecretsError> {
        let path = key.to_key_str();
        self.writer_for_path(path.as_ref())
            .create_credentials(key, credentials)
            .await
    }

    async fn delete_credentials(&self, key: &CredentialKey) -> Result<(), SecretsError> {
        let path = key.to_key_str();
        self.writer_for_path(path.as_ref())
            .delete_credentials(key)
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use carbide_uuid::rack::RackId;
    use carbide_uuid::machine::MachineId;

    use super::*;
    use crate::credentials::{CredentialKey, CredentialReader, Credentials};
    use crate::test_support::credentials::TestCredentialManager;

    fn cred(user: &str, pass: &str) -> Credentials {
        Credentials::UsernamePassword {
            username: user.to_string(),
            password: pass.to_string(),
        }
    }

    fn routed_writer(
        postgres: Arc<TestCredentialManager>,
        vault: Arc<TestCredentialManager>,
    ) -> RoutedCredentialWriter {
        RoutedCredentialWriter::new(
            vec![
                ("/".to_string(), vault.clone() as Arc<dyn CredentialWriter>),
                (
                    "racks".to_string(),
                    postgres.clone() as Arc<dyn CredentialWriter>,
                ),
            ],
            vault,
        )
    }

    #[tokio::test]
    async fn rack_token_routes_to_postgres_writer() {
        let postgres = Arc::new(TestCredentialManager::default());
        let vault = Arc::new(TestCredentialManager::default());
        let writer = routed_writer(postgres.clone(), vault.clone());

        let rack_id = RackId::new("launchpad-r1");
        let key = CredentialKey::RackMaintenanceAccessToken {
            rack_id: rack_id.clone(),
        };
        writer
            .set_credentials(&key, &cred("access_token", "HELLO"))
            .await
            .expect("set rack token");

        assert!(
            postgres
                .get_credentials(&key)
                .await
                .expect("postgres read")
                .is_some(),
            "rack token should be stored in postgres writer"
        );
        assert!(
            vault.get_credentials(&key).await.expect("vault read").is_none(),
            "vault writer should not receive rack token"
        );
    }

    #[tokio::test]
    async fn non_rack_credential_routes_to_vault_writer() {
        let postgres = Arc::new(TestCredentialManager::default());
        let vault = Arc::new(TestCredentialManager::default());
        let writer = routed_writer(postgres.clone(), vault.clone());

        let key = CredentialKey::UfmAuth {
            fabric: "fabric-a".to_string(),
        };
        writer
            .set_credentials(&key, &cred("admin", "secret"))
            .await
            .expect("set ufm cred");

        assert!(
            vault.get_credentials(&key).await.expect("vault read").is_some(),
            "non-rack credential should be stored in vault writer"
        );
        assert!(
            postgres
                .get_credentials(&key)
                .await
                .expect("postgres read")
                .is_none(),
            "postgres writer should not receive non-rack credential"
        );
    }

    #[tokio::test]
    async fn delete_routes_same_as_write() {
        let postgres = Arc::new(TestCredentialManager::default());
        let vault = Arc::new(TestCredentialManager::default());
        let writer = routed_writer(postgres.clone(), vault.clone());

        let rack_id = RackId::new("rack-01");
        let key = CredentialKey::RackMaintenanceAccessToken { rack_id };
        writer
            .set_credentials(&key, &cred("access_token", "token"))
            .await
            .expect("set");
        writer.delete_credentials(&key).await.expect("delete");

        assert!(
            postgres.get_credentials(&key).await.expect("read").is_none(),
            "delete should remove rack token from postgres writer"
        );
    }

    #[tokio::test]
    async fn prefix_does_not_match_mid_segment() {
        let postgres = Arc::new(TestCredentialManager::default());
        let vault = Arc::new(TestCredentialManager::default());
        let writer = RoutedCredentialWriter::new(
            vec![(
                "racks".to_string(),
                postgres.clone() as Arc<dyn CredentialWriter>,
            )],
            vault.clone() as Arc<dyn CredentialWriter>,
        );

        let key = CredentialKey::DpuSsh {
            #[allow(deprecated)]
            machine_id: MachineId::default(),
        };
        writer
            .set_credentials(&key, &cred("user", "pass"))
            .await
            .expect("set");

        assert!(
            vault.get_credentials(&key).await.expect("read").is_some(),
            "machines/ path must not match racks/ prefix"
        );
    }

    #[test]
    fn normalize_prefix_adds_trailing_slash() {
        assert_eq!(normalize_prefix("racks"), "racks/");
        assert_eq!(normalize_prefix("racks/"), "racks/");
        assert_eq!(normalize_prefix("/"), "");
    }
}
