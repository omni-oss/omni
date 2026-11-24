use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::core::{
    BackendSupport, Catalog, CatalogId, Chain, ChainBinding, ChainName,
    Direction, Error, GuardedObservation, MigrationSession, MigrationStore,
    Plan, Policy, Revision, Stamp, Step, StepCapabilities, StepOutcome,
    StoreId, StoreState, Transition, Version, build_plan, check_backend,
};
use crate::fs::{DurableFs, Faults, Guard, PublishError, fingerprint};

pub const WITH_INDEX: Version = Version(90);
pub const WITHOUT_INDEX: Version = Version(12);
pub const INDEX_CATALOG: CatalogId = CatalogId(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IndexStep {
    DropShards,
    RebuildShards,
}

pub const INDEX_EDGES: &[Transition<IndexStep>] = &[Transition {
    previous: WITH_INDEX,
    version: WITHOUT_INDEX,
    upgrade: IndexStep::DropShards,
    downgrade: Some(IndexStep::RebuildShards),
}];

const SHARDS: [&str; 2] = ["shard-0", "shard-1"];

fn index_version(tag: Option<&Value>) -> Result<Version, Error> {
    match tag {
        None => Err(Error::MissingVersion),
        Some(Value::String(s)) => match s.as_str() {
            "indexed" => Ok(WITH_INDEX),
            "records-only" => Ok(WITHOUT_INDEX),
            s if s.chars().all(|c| c.is_ascii_lowercase() || c == '-') => {
                Err(Error::UnknownTag)
            }
            _ => Err(Error::MalformedVersion),
        },
        Some(_) => Err(Error::MalformedVersion),
    }
}

fn tag_of(v: Version) -> &'static str {
    if v == WITH_INDEX {
        "indexed"
    } else {
        "records-only"
    }
}

/// Fixed derivation definition: shard 0 holds even-length records, shard 1
/// odd-length, each sorted and newline-joined.
pub fn derive(records: &[String]) -> Vec<(&'static str, Vec<u8>)> {
    let mut sorted = records.to_vec();
    sorted.sort();
    SHARDS
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let lines: Vec<&str> = sorted
                .iter()
                .filter(|r| r.len() % 2 == i)
                .map(String::as_str)
                .collect();
            (*name, lines.join("\n").into_bytes())
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Marker {
    version: Version,
    revision: u64,
}

impl Marker {
    fn to_json(self) -> Value {
        json!({"version": tag_of(self.version), "revision": self.revision})
    }

    fn bytes(self) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "chain": "index",
            "version": tag_of(self.version),
            "revision": self.revision,
        }))
        .unwrap()
    }

    fn from_json(v: &Value) -> Result<Self, Error> {
        Ok(Self {
            version: index_version(v.get("version"))
                .map_err(|_| Error::MalformedIntent)?,
            revision: v
                .get("revision")
                .and_then(Value::as_u64)
                .ok_or(Error::MalformedIntent)?,
        })
    }
}

struct Records {
    incarnation: u64,
    generation: u64,
    records: Vec<String>,
    fp: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexRecovered {
    Target,
    RolledBack,
    RollbackVerified,
    Records,
    Nothing,
}

#[derive(Clone, Debug)]
pub struct IndexCatalog {
    pub backend: BackendSupport,
    pub edges: Vec<Transition<IndexStep>>,
}

impl IndexCatalog {
    pub fn standard() -> Self {
        Self {
            backend: BackendSupport::DURABLE_NON_ATOMIC,
            edges: INDEX_EDGES.to_vec(),
        }
    }

    pub fn chain(&self) -> Result<Chain<'_, IndexStep>, Error> {
        Chain::new(WITH_INDEX, &self.edges)
    }
}

impl Catalog<IndexStep> for IndexCatalog {
    fn id(&self) -> CatalogId {
        INDEX_CATALOG
    }

    fn binding(&self) -> ChainBinding {
        self.chain()
            .expect("valid fixture chain")
            .binding(ChainName::INDEX)
    }

    fn backend(&self) -> BackendSupport {
        self.backend
    }

    fn capabilities(&self, s: Step<IndexStep>) -> Option<StepCapabilities> {
        match (s.from, s.to, s.id) {
            (WITH_INDEX, WITHOUT_INDEX, IndexStep::DropShards) => {
                Some(StepCapabilities {
                    atomic: false,
                    lossless: false,
                    derivable: true,
                })
            }
            (WITHOUT_INDEX, WITH_INDEX, IndexStep::RebuildShards) => {
                Some(StepCapabilities {
                    atomic: false,
                    lossless: true,
                    derivable: false,
                })
            }
            _ => None,
        }
    }
}

pub struct IndexStore {
    root: PathBuf,
    fs: DurableFs,
}

impl IndexStore {
    pub fn open(root: &Path, faults: Faults) -> Self {
        Self {
            root: root.to_path_buf(),
            fs: DurableFs::new(faults),
        }
    }

    pub fn create(
        root: &Path,
        faults: Faults,
        incarnation: u64,
        version: Version,
        records: &[&str],
    ) -> Result<Self, Error> {
        let store = Self::open(root, faults);
        let records: Vec<String> =
            records.iter().map(|s| s.to_string()).collect();
        store.write_records(incarnation, 1, &records)?;
        if version == WITH_INDEX {
            store.restore_layout(version, &records)?;
        }
        store
            .fs
            .write_atomic(
                &store.marker_path(),
                &Marker {
                    version,
                    revision: 1,
                }
                .bytes(),
                "create",
            )
            .map_err(PublishError::into_error)?;
        Ok(store)
    }

    pub fn faults(&self) -> &Faults {
        &self.fs.faults
    }

    pub fn marker_path(&self) -> PathBuf {
        self.root.join("marker.json")
    }

    pub fn records_path(&self) -> PathBuf {
        self.root.join("records.json")
    }

    pub fn shards_dir(&self) -> PathBuf {
        self.root.join("shards")
    }

    pub fn intent_path(&self) -> PathBuf {
        self.root.join(".migration.intent")
    }

    pub fn guard_path(&self) -> PathBuf {
        self.root.join(".migration.guard")
    }

    pub fn planning_catalog(&self) -> IndexCatalog {
        let mut c = IndexCatalog::standard();
        c.backend = self.fs.probe();
        c
    }

    fn write_records(
        &self,
        incarnation: u64,
        generation: u64,
        records: &[String],
    ) -> Result<(), Error> {
        let bytes = serde_json::to_vec(&json!({
            "incarnation": incarnation,
            "generation": generation,
            "records": records,
        }))
        .unwrap();
        self.fs
            .write_atomic(&self.records_path(), &bytes, "records")
            .map_err(PublishError::into_error)
    }

    fn read_records(&self) -> Result<Records, Error> {
        let bytes = std::fs::read(self.records_path())?;
        let v: Value = serde_json::from_slice(&bytes)
            .map_err(|e| Error::MalformedDocument(e.to_string()))?;
        let bad = || Error::MalformedDocument("records".into());
        Ok(Records {
            incarnation: v
                .get("incarnation")
                .and_then(Value::as_u64)
                .ok_or_else(bad)?,
            generation: v
                .get("generation")
                .and_then(Value::as_u64)
                .ok_or_else(bad)?,
            records: serde_json::from_value(
                v.get("records").cloned().ok_or_else(bad)?,
            )
            .map_err(|_| bad())?,
            fp: fingerprint(&bytes),
        })
    }

    fn read_marker(&self) -> Result<Marker, Error> {
        let bytes = match std::fs::read(self.marker_path()) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::MissingVersion);
            }
            Err(e) => return Err(e.into()),
        };
        let v: Value = serde_json::from_slice(&bytes)
            .map_err(|_| Error::MalformedVersion)?;
        match v.get("chain") {
            None => return Err(Error::MissingChain),
            Some(Value::String(s)) if s == ChainName::INDEX.0 => {}
            Some(Value::String(_)) => return Err(Error::ChainMismatch),
            Some(_) => return Err(Error::MalformedChain),
        }
        let version = index_version(v.get("version"))?;
        let revision = v
            .get("revision")
            .and_then(Value::as_u64)
            .ok_or(Error::MalformedVersion)?;
        Ok(Marker { version, revision })
    }

    fn shard_files(&self) -> Result<Vec<(String, Vec<u8>)>, Error> {
        let dir = self.shards_dir();
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut out = vec![];
        for e in std::fs::read_dir(dir)? {
            let e = e?;
            out.push((
                e.file_name().to_string_lossy().into_owned(),
                std::fs::read(e.path())?,
            ));
        }
        out.sort();
        Ok(out)
    }

    pub fn shard_bytes(&self) -> Result<Vec<(String, Vec<u8>)>, Error> {
        self.shard_files()
    }

    fn verify_layout(
        &self,
        v: Version,
        records: &[String],
    ) -> Result<(), Error> {
        let found = self.shard_files()?;
        if v == WITH_INDEX {
            let want: Vec<(String, Vec<u8>)> = derive(records)
                .into_iter()
                .map(|(n, b)| (n.to_string(), b))
                .collect();
            if found != want {
                return Err(Error::Inconsistent(
                    "shards differ from records".into(),
                ));
            }
        } else if !found.is_empty() {
            return Err(Error::Inconsistent(
                "shards present without index".into(),
            ));
        }
        Ok(())
    }

    fn restore_layout(
        &self,
        v: Version,
        records: &[String],
    ) -> Result<(), Error> {
        let dir = self.shards_dir();
        if v == WITH_INDEX {
            std::fs::create_dir_all(&dir)?;
            for (name, bytes) in derive(records) {
                self.faults().hit("shard.write")?;
                self.fs.faults.mutate();
                std::fs::write(dir.join(name), bytes)?;
            }
            for (name, _) in self.shard_files()? {
                if !SHARDS.contains(&name.as_str()) {
                    std::fs::remove_file(dir.join(name))?;
                }
            }
        } else if dir.exists() {
            for (name, _) in self.shard_files()? {
                self.faults().hit("shard.remove")?;
                self.fs.faults.mutate();
                std::fs::remove_file(dir.join(name))?;
                self.faults().hit("shard.removed")?;
            }
        }
        Ok(())
    }

    fn sync_layout(&self) -> Result<(), Error> {
        let dir = self.shards_dir();
        if dir.exists() {
            for (name, _) in self.shard_files()? {
                std::fs::File::open(dir.join(name))?.sync_all()?;
            }
            self.fs.sync_dir(&dir)?;
        }
        self.fs.sync_dir(&self.root)
    }

    fn pending_intent(&self) -> Result<Option<Value>, Error> {
        let bytes = match std::fs::read(self.intent_path()) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(e) => return Err(e.into()),
        };
        let v: Value = serde_json::from_slice(&bytes)
            .map_err(|_| Error::MalformedIntent)?;
        match v.get("chain") {
            Some(Value::String(_)) => Ok(Some(v)),
            _ => Err(Error::MalformedIntent),
        }
    }

    fn pending_intent_chain(&self) -> Result<Option<String>, Error> {
        Ok(self
            .pending_intent()?
            .map(|v| v["chain"].as_str().unwrap().to_string()))
    }

    pub fn intent_exists(&self) -> bool {
        self.intent_path().exists()
    }

    pub fn read_marker_version(&self) -> Result<Version, Error> {
        Ok(self.read_marker()?.version)
    }

    pub fn observe(&self) -> Result<GuardedObservation, Error> {
        if let Some(name) = self.pending_intent_chain()? {
            if name != ChainName::INDEX.0 {
                return Err(Error::ChainMismatch);
            }
            return Ok(GuardedObservation::RecoveryRequired);
        }
        let marker = self.read_marker()?;
        let records = self.read_records()?;
        self.verify_layout(marker.version, &records.records)?;
        let rev = fingerprint(
            format!("{}:{}", records.generation, marker.revision).as_bytes(),
        );
        Ok(GuardedObservation::Ready(Stamp {
            store: StoreId(records.incarnation),
            chain: ChainName::INDEX,
            revision: Revision(u64::from_str_radix(&rev[..16], 16).unwrap()),
            version: marker.version,
        }))
    }

    pub fn state(&self) -> Result<StoreState, Error> {
        Ok(match self.observe()? {
            GuardedObservation::Ready(s) => StoreState::Ready(s.version),
            GuardedObservation::RecoveryRequired => {
                StoreState::RecoveryRequired
            }
        })
    }

    pub fn plan(
        &self,
        target: Version,
        direction: Direction,
        policy: Policy,
    ) -> Result<Plan<IndexStep>, Error> {
        let stamp = match self.observe()? {
            GuardedObservation::Ready(s) => s,
            GuardedObservation::RecoveryRequired => return Err(Error::Dirty),
        };
        let catalog = self.planning_catalog();
        build_plan(
            &catalog.chain()?,
            &catalog,
            stamp,
            target,
            direction,
            policy,
        )
    }

    /// Records-only writer: refuses a pending intent and publishes its own.
    pub fn edit_records(&self, add: &str) -> Result<(), Error> {
        let _guard = self.fs.lock_exclusive(&self.guard_path())?;
        if self.pending_intent()?.is_some() {
            return Err(Error::Dirty);
        }
        if self.read_marker()?.version == WITH_INDEX {
            return Err(Error::Inconsistent(
                "editor needs records-only".into(),
            ));
        }
        let old = self.read_records()?;
        let mut records = old.records.clone();
        records.push(add.to_string());
        let next = serde_json::to_vec(&json!({
            "incarnation": old.incarnation,
            "generation": old.generation + 1,
            "records": records,
        }))
        .unwrap();
        let intent = serde_json::to_vec(&json!({
            "chain": "index",
            "kind": "records",
            "old": old.fp,
            "new": fingerprint(&next),
        }))
        .unwrap();
        self.fs
            .write_atomic(&self.intent_path(), &intent, "intent")
            .map_err(PublishError::into_error)?;
        self.fs
            .write_atomic(&self.records_path(), &next, "records")
            .map_err(PublishError::into_error)?;
        self.fs
            .remove_and_sync(&self.intent_path(), "intent.clear")?;
        Ok(())
    }

    pub fn recover_under_guard(&self) -> Result<IndexRecovered, Error> {
        let _guard = self.fs.lock_exclusive(&self.guard_path())?;
        let Some(intent) = self.pending_intent()? else {
            return Ok(IndexRecovered::Nothing);
        };
        if intent["chain"] != ChainName::INDEX.0 {
            return Err(Error::ChainMismatch);
        }
        let records =
            self.read_records().map_err(|_| Error::RecoveryBlocked)?;
        let clear = |this: &Self| -> Result<(), Error> {
            if this
                .fs
                .remove_and_sync(&this.intent_path(), "intent.clear")?
            {
                Ok(())
            } else {
                Err(Error::Io("intent removal not synced".into()))
            }
        };
        if intent.get("kind").and_then(Value::as_str) == Some("records") {
            let live = records.fp.as_str();
            if intent["old"] != live && intent["new"] != live {
                return Err(Error::RecoveryBlocked);
            }
            std::fs::File::open(self.records_path())?.sync_all()?;
            self.fs.sync_dir(&self.root)?;
            clear(self)?;
            return Ok(IndexRecovered::Records);
        }
        if intent["records_fp"] != records.fp.as_str() {
            return Err(Error::RecoveryBlocked);
        }
        let old = Marker::from_json(&intent["old"])?;
        let target = Marker::from_json(&intent["target"])?;
        let rollback = Marker::from_json(&intent["rollback"])?;
        let live = self.read_marker().map_err(|_| Error::RecoveryBlocked)?;
        let blocked = |e: Error| match e {
            Error::Inconsistent(_) => Error::RecoveryBlocked,
            e => e,
        };
        let outcome = if live == target {
            self.verify_layout(target.version, &records.records)
                .map_err(blocked)?;
            IndexRecovered::Target
        } else if live == old {
            self.restore_layout(old.version, &records.records)?;
            self.verify_layout(old.version, &records.records)
                .map_err(blocked)?;
            self.sync_layout()?;
            self.fs
                .write_atomic(&self.marker_path(), &rollback.bytes(), "marker")
                .map_err(PublishError::into_error)?;
            IndexRecovered::RolledBack
        } else if live == rollback {
            self.verify_layout(rollback.version, &records.records)
                .map_err(blocked)?;
            IndexRecovered::RollbackVerified
        } else {
            return Err(Error::RecoveryBlocked);
        };
        self.sync_layout()?;
        std::fs::File::open(self.marker_path())?.sync_all()?;
        clear(self)?;
        Ok(outcome)
    }
}

pub struct IndexSession<'a> {
    store: &'a IndexStore,
    catalog: IndexCatalog,
    _guard: Guard,
}

impl MigrationStore<IndexStep> for IndexStore {
    type Session<'a> = IndexSession<'a>;

    fn begin_migration(&self) -> Result<IndexSession<'_>, Error> {
        let guard = self.fs.lock_exclusive(&self.guard_path())?;
        Ok(IndexSession {
            store: self,
            catalog: self.planning_catalog(),
            _guard: guard,
        })
    }
}

impl Catalog<IndexStep> for IndexSession<'_> {
    fn id(&self) -> CatalogId {
        self.catalog.id()
    }

    fn binding(&self) -> ChainBinding {
        self.catalog.binding()
    }

    fn backend(&self) -> BackendSupport {
        self.catalog.backend()
    }

    fn capabilities(&self, s: Step<IndexStep>) -> Option<StepCapabilities> {
        self.catalog.capabilities(s)
    }
}

impl MigrationSession<IndexStep> for IndexSession<'_> {
    fn probe(&self) -> BackendSupport {
        self.store.fs.probe()
    }

    fn observe(&self) -> Result<GuardedObservation, Error> {
        self.store.observe()
    }

    fn apply(&mut self, s: Step<IndexStep>) -> StepOutcome {
        use StepOutcome::{Committed, NotCommitted, RecoveryRequired};
        let st = self.store;
        let Some(c) = self.capabilities(s) else {
            return NotCommitted(Error::MissingStep);
        };
        if let Err(e) = check_backend(st.fs.probe(), c) {
            return NotCommitted(e);
        }
        let prepared = (|| {
            let marker = st.read_marker()?;
            let records = st.read_records()?;
            if marker.version != s.from {
                return Err(Error::UnexpectedVersion);
            }
            st.verify_layout(s.from, &records.records)?;
            Ok((marker, records))
        })();
        let (marker, records) = match prepared {
            Ok(v) => v,
            // Only a wrong version leaves the old layout proven healthy;
            // unreadable or inconsistent state fails closed.
            Err(Error::UnexpectedVersion) => {
                return NotCommitted(Error::UnexpectedVersion);
            }
            Err(e) => return RecoveryRequired(e),
        };
        let target = Marker {
            version: s.to,
            revision: marker.revision + 1,
        };
        let rollback = Marker {
            version: s.from,
            revision: marker.revision + 2,
        };
        let intent = serde_json::to_vec(&json!({
            "chain": "index",
            "from": tag_of(s.from),
            "to": tag_of(s.to),
            "records_fp": records.fp,
            "old": marker.to_json(),
            "target": target.to_json(),
            "rollback": rollback.to_json(),
        }))
        .unwrap();
        match st.fs.write_atomic(&st.intent_path(), &intent, "intent") {
            Ok(()) => {}
            Err(PublishError::BeforeRename(e)) => return NotCommitted(e),
            Err(PublishError::Uncertain(e)) => return RecoveryRequired(e),
        }
        let result = st
            .restore_layout(s.to, &records.records)
            .and_then(|_| st.verify_layout(s.to, &records.records))
            .and_then(|_| st.sync_layout())
            .and_then(|_| {
                st.fs
                    .write_atomic(&st.marker_path(), &target.bytes(), "marker")
                    .map_err(PublishError::into_error)
            })
            .and_then(|_| {
                st.fs
                    .remove_and_sync(&st.intent_path(), "intent.clear")
                    .map(|_| ())
            });
        match result {
            Ok(()) => Committed,
            Err(e) => RecoveryRequired(e),
        }
    }
}
