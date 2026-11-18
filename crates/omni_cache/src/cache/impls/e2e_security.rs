// End to end security scenarios for RFC 0030: the real hybrid store talking
// HTTP to the real server router over a socket, with in-memory storage.
//
// Tokens are `principal|scopes|pin` (see [`Tokens`]).

use std::sync::Arc;

use async_trait::async_trait;
use omni_configurations::SigningKeyConfiguration;
use omni_remote_cache_core::{
    config::Configuration,
    data::{
        environments::DynEnvironmentRepository,
        organizations::DynOrganizationRepository, tenants::DynTenantRepository,
        workspaces::DynWorkspaceRepository,
    },
    entry::{EntryRead, SealParams, TrailerCredential, read_entry, seal},
    providers::{ConfigBasedDependencyProvider, DependencyProvider},
    security::{
        AuthError, Authenticator, AuthenticatorChain, Caller, CredentialId,
        CredentialKind, Resource, Scope, Scopes,
    },
    services::DynValidationService,
    state::{ServiceInfo, ServiceState},
};
use omni_remote_cache_storage_core::{
    Container, RemoteCacheStorageBackend as _, SaveOutcome,
};
use omni_remote_cache_storage_memory::InMemoryBackend;

use crate::impls::signing_policy::SigningPolicy;

struct Tokens;

#[async_trait]
impl Authenticator for Tokens {
    async fn authenticate(
        &self,
        credential: &str,
        _: &Resource,
        wanted: Scopes,
    ) -> Result<Option<Caller>, AuthError> {
        let mut parts = credential.split('|');
        let (Some(principal), Some(letters), Some(_pin), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Ok(None);
        };
        let held: Scopes = letters
            .chars()
            .map(|c| match c {
                'r' => Scope::Read,
                'w' => Scope::Write,
                'p' => Scope::Publish,
                'l' => Scope::List,
                _ => Scope::Delete,
            })
            .collect();
        Caller::new(
            principal.into(),
            CredentialId::new(CredentialKind::Custom("t".into()), principal.into())?,
            held.intersection(wanted),
            None,
        )
        .map(Some)
    }
}

struct Provider {
    inner: ConfigBasedDependencyProvider,
}

impl DependencyProvider for Provider {
    fn authenticator(&self) -> Arc<dyn Authenticator> {
        Arc::new(AuthenticatorChain::new(vec![Arc::new(Tokens)]))
    }
    fn workspace_repository(&self) -> DynWorkspaceRepository {
        self.inner.workspace_repository()
    }
    fn organization_repository(&self) -> DynOrganizationRepository {
        self.inner.organization_repository()
    }
    fn environment_repository(&self) -> DynEnvironmentRepository {
        self.inner.environment_repository()
    }
    fn tenant_repository(&self) -> DynTenantRepository {
        self.inner.tenant_repository()
    }
    fn validation_service(&self) -> DynValidationService {
        self.inner.validation_service()
    }
}

struct Remote {
    url: String,
    backend: Arc<InMemoryBackend>,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for Remote {
    fn drop(&mut self) {
        self.server.abort();
    }
}

async fn remote() -> Remote {
    let config = Configuration::from_inline(
        serde_json::json!({
            "tenants": { "t": { "organizations": { "o": { "workspaces": {
                "w": { "environments": { "e": {} } }
            } } } } },
            "security": { "api_keys": {} }
        })
        .to_string(),
    )
    .unwrap();
    let backend = Arc::new(InMemoryBackend::new());
    let provider = Arc::new(Provider {
        inner: ConfigBasedDependencyProvider::new(
            Arc::new(config.to_in_memory_database()),
            Arc::new(config.security.api_keys),
        ),
    });
    let state = ServiceState::new(
        backend.clone(),
        Arc::new(ServiceInfo::new("test", "0", "", false, Some("/api".into()))),
        provider,
    );
    let router = omni_remote_cache_core::routes::root::build_router(
        &omni_remote_cache_core::routes::root::RouterConfig::new(
            Some("/api".into()),
            false,
        ),
    )
    .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/api", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    Remote {
        url,
        backend,
        server,
    }
}

struct Key {
    trusted: omni_remote_cache_client::TrustedKey,
    private_key: String,
    id: String,
}

fn key() -> Key {
    let kp = omni_remote_cache_client::generate_keypair().unwrap();
    Key {
        trusted: omni_remote_cache_client::TrustedKey::parse(&kp.id, &kp.public_key).unwrap(),
        private_key: kp.private_key.to_string(),
        id: kp.id,
    }
}

/// A store for one workspace and one credential.
struct Client {
    store: HybridTaskExecutionCacheStore,
}

fn client(
    temp: &tempfile::TempDir,
    remote: &Remote,
    token: &str,
    push: bool,
    target: RemoteTarget,
    policy: SigningPolicy,
) -> Client {
    let store = HybridTaskExecutionCacheStore::new(
        temp.path().join(".omni/cache-e2e"),
        temp.path(),
        RemoteConfig::Enabled(EnabledRemoteConfig {
            api_base_url: remote.url.clone(),
            api_key: token.to_string(),
            tenant_code: "t".to_string(),
            organization_code: "o".to_string(),
            workspace_code: "w".to_string(),
            environment_code: Some("e".to_string()),
            push,
            target,
            scope: ScopeConfig::Literal("pr-1".to_string()),
            max_artifact_size: None,
            signing: Box::new(policy),
        }),
    );
    Client { store }
}

fn policy(trusted: &[&Key], mode: EffectiveVerify) -> SigningPolicy {
    SigningPolicy {
        verify: mode,
        trusted: trusted.iter().map(|k| k.trusted.clone()).collect(),
        signing_key: None,
    }
}

fn publisher_policy(key: &Key, dir: &Path) -> SigningPolicy {
    let file = dir.join("signing-key");
    std::fs::write(&file, &key.private_key).unwrap();
    SigningPolicy {
        signing_key: Some(SigningKeyConfiguration {
            id: key.id.clone(),
            private_key_env: None,
            private_key_file: Some(file),
        }),
        ..policy(&[key], EffectiveVerify::Require)
    }
}

async fn build_and_upload(temp: &tempfile::TempDir, client: &Client) {
    let t0 = task("task", "project1", temp.path());
    client
        .store
        .cache(&new_cache_info(Some(&LOGS_CONTENT), *t0.get()))
        .await
        .unwrap();
}

async fn restores(temp: &tempfile::TempDir, client: &Client) -> bool {
    let t0 = task("task", "project1", temp.path());
    client.store.get(t0.get()).await.unwrap().is_some()
}

async fn objects(remote: &Remote, container: &str) -> Vec<String> {
    remote
        .backend
        .list(&Container::parse(container).unwrap())
        .await
        .unwrap()
        .into_iter()
        .map(|i| i.key)
        .collect()
}

#[tokio::test]
async fn creep_a_poisoned_pr_artifact_is_never_restored_by_a_trusted_build() {
    let remote = remote().await;
    let release = key();

    // A pull request job holds only `write`: it can fill its own isolated
    // namespace and nothing else.
    let pr = fixture(&["project1"]).await;
    let pr_client = client(&pr, &remote, "pr-job|rw|-", true, RemoteTarget::Isolated, SigningPolicy::default());
    build_and_upload(&pr, &pr_client).await;
    assert!(objects(&remote, "shared/t/o/w/e").await.is_empty());
    let isolated = objects(&remote, "isolated/t/o/w/e/pr-job/pr-1").await;
    assert_eq!(isolated.len(), 1, "the PR's entry lives in its own namespace");

    // The PR job cannot publish: the server refuses the shared upload.
    let pr2 = fixture(&["project1"]).await;
    let sneaky = client(&pr2, &remote, "pr-job|rw|-", true, RemoteTarget::Shared, SigningPolicy::default());
    build_and_upload(&pr2, &sneaky).await;
    assert!(objects(&remote, "shared/t/o/w/e").await.is_empty());

    // The trusted `main` build, with a different credential, misses.
    let main = fixture(&["project1"]).await;
    let main_client = client(&main, &remote, "main-job|rwp|-", false, RemoteTarget::Shared, policy(&[&release], EffectiveVerify::Require));
    assert!(!restores(&main, &main_client).await, "the PR artifact must not be restored");

    // Even a storage-level copy of the PR's object into `shared/` is a miss:
    // the entry says it is isolated and it is unsigned.
    let raw = remote
        .backend
        .get(&Container::parse("isolated/t/o/w/e/pr-job/pr-1").unwrap(), &isolated[0])
        .await
        .unwrap()
        .unwrap();
    remote
        .backend
        .save_if_absent(&Container::parse("shared/t/o/w/e").unwrap(), &isolated[0], raw)
        .await
        .unwrap();
    let main2 = fixture(&["project1"]).await;
    let main_client2 = client(&main2, &remote, "main-job|rwp|-", false, RemoteTarget::Shared, policy(&[&release], EffectiveVerify::Require));
    assert!(!restores(&main2, &main_client2).await);
    assert!(markers_in(&main2).is_empty(), "nothing was extracted");
}

#[tokio::test]
async fn a_signed_publish_is_restored_by_a_requiring_build_and_only_then() {
    let remote = remote().await;
    let release = key();
    let publisher = fixture(&["project1"]).await;
    let publisher_client = client(
        &publisher,
        &remote,
        "release-job|rwp|-",
        true,
        RemoteTarget::Shared,
        publisher_policy(&release, publisher.path()),
    );
    build_and_upload(&publisher, &publisher_client).await;
    assert_eq!(objects(&remote, "shared/t/o/w/e").await.len(), 1);

    // A requiring reader that trusts the release key restores it.
    let reader = fixture(&["project1"]).await;
    let reader_client = client(&reader, &remote, "dev|r|-", false, RemoteTarget::Isolated, policy(&[&release], EffectiveVerify::Require));
    assert!(restores(&reader, &reader_client).await);

    // A reader that trusts only some other key does not.
    let stranger = key();
    let other = fixture(&["project1"]).await;
    let other_client = client(&other, &remote, "dev|r|-", false, RemoteTarget::Isolated, policy(&[&stranger], EffectiveVerify::Require));
    assert!(!restores(&other, &other_client).await);
}

#[tokio::test]
async fn server_compromise_tampered_bytes_with_a_stolen_signature_are_a_miss() {
    let remote = remote().await;
    let release = key();
    let publisher = fixture(&["project1"]).await;
    let publisher_client = client(
        &publisher,
        &remote,
        "release-job|rwp|-",
        true,
        RemoteTarget::Shared,
        publisher_policy(&release, publisher.path()),
    );
    build_and_upload(&publisher, &publisher_client).await;
    let container = Container::parse("shared/t/o/w/e").unwrap();
    let digest = objects(&remote, "shared/t/o/w/e").await.remove(0);
    let EntryRead::Valid(original) =
        read_entry(remote.backend.as_ref(), &container, &digest).await.unwrap()
    else {
        panic!("the publisher stored a sealed entry")
    };
    let stolen_signature = original.parsed.trailer.signature.clone();
    assert!(stolen_signature.is_some(), "the publisher signed it");

    // The attacker controls the server: it replaces the stored object with a
    // different, perfectly sealed archive that carries the real signature.
    // Preserve valid metadata so the regression exercises signature checking,
    // not rejection of an incomplete archive before publication.
    let task = task("task", "project1", publisher.path());
    let genuine = genuine_archive(&publisher, task.get()).await;
    let evil = {
        let mut out = Vec::new();
        let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
            &mut out,
            flate2::Compression::default(),
        ));
        let mut source = tar::Archive::new(flate2::read::GzDecoder::new(&genuine[..]));
        for entry in source.entries().unwrap() {
            let mut entry = entry.unwrap();
            tar.append(&entry.header().clone(), &mut entry).unwrap();
        }
        let mut header = tar::Header::new_gnu();
        let payload = b"attacker controlled";
        header.set_size(payload.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, "evil.txt", &payload[..]).unwrap();
        tar.into_inner().unwrap().finish().unwrap();
        out
    };
    remote.backend.delete(&container, &digest).await.unwrap();
    let (stream, _) = seal(
        Box::pin(futures::stream::iter(vec![Ok::<_, omni_remote_cache_storage_core::error::Error>(
            bytes::Bytes::from(evil.clone()),
        )])),
        SealParams {
            declared_sha256: {
                use sha2::{Digest as _, Sha256};
                Sha256::digest(&evil).iter().map(|b| format!("{b:02x}")).collect()
            },
            limit: u64::MAX,
            writer_credential: TrailerCredential {
                kind: "api_key".into(),
                id: "attacker".into(),
            },
            writer_principal: "attacker".into(),
            namespace: "shared".into(),
            scope: None,
            signature: stolen_signature,
            now: time::OffsetDateTime::now_utc,
        },
    );
    assert_eq!(
        remote
            .backend
            .save_stream_if_absent(&container, &digest, stream)
            .await
            .unwrap(),
        SaveOutcome::Stored
    );

    // A requiring reader misses and extracts nothing.
    let reader = fixture(&["project1"]).await;
    let reader_client = client(&reader, &remote, "dev|r|-", false, RemoteTarget::Isolated, policy(&[&release], EffectiveVerify::Require));
    assert!(!restores(&reader, &reader_client).await);
    assert!(markers_in(&reader).is_empty());
    // The same tampering is not noticed when nobody verifies, which is what the
    // signature is for (this proves the scenario really replaces the bytes).
    let naive = fixture(&["project1"]).await;
    let naive_client = client(&naive, &remote, "dev|r|-", false, RemoteTarget::Isolated, policy(&[], EffectiveVerify::Off));
    let _ = restores(&naive, &naive_client).await;
    assert_eq!(
        markers_in(&naive).len(),
        1,
        "without verification the attacker's archive is extracted"
    );
}

/// Origin markers of everything restored in `temp`'s cache.
fn markers_in(temp: &tempfile::TempDir) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![temp.path().join(".omni/cache-e2e")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n == ".omni-origin-remote") {
                out.push(path.display().to_string());
            }
        }
    }
    out
}
