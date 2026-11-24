use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::core::{
    BackendSupport, Catalog, CatalogId, Chain, ChainBinding, ChainName,
    Direction, Error, GuardedObservation, MigrationSession, MigrationStore,
    Plan, Policy, Revision, Stamp, Step, StepCapabilities, StepOutcome,
    StoreId, StoreState, Transition, Version, build_plan, check_backend,
};
use crate::fs::{DurableFs, Faults, Guard, PublishError, fingerprint};

pub const PINS_V1: Version = Version(7);
pub const PINS_V2: Version = Version(42);
pub const PINS_V3: Version = Version(13);
pub const PINS_CATALOG: CatalogId = CatalogId(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PinsStep {
    AddNotes,
    BackToPlainPins,
    AddTags,
    DropTags,
}

pub fn pins_version(tag: Option<&Value>) -> Result<Version, Error> {
    match tag {
        None => Err(Error::MissingVersion),
        Some(Value::String(s)) => match s.as_str() {
            "pins-v1" => Ok(PINS_V1),
            "pins-v2" => Ok(PINS_V2),
            "pins-v3" => Ok(PINS_V3),
            _ if well_formed_pin_tag(s) => Err(Error::UnknownTag),
            _ => Err(Error::MalformedVersion),
        },
        Some(_) => Err(Error::MalformedVersion),
    }
}

fn well_formed_pin_tag(s: &str) -> bool {
    s.strip_prefix("pins-v")
        .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
}

fn tag_of(v: Version) -> &'static str {
    match v {
        PINS_V1 => "pins-v1",
        PINS_V2 => "pins-v2",
        PINS_V3 => "pins-v3",
        _ => unreachable!("fixture version"),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V1 {
    pub pins: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V2 {
    pub pins: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V3 {
    pub pins: Vec<String>,
    pub notes: Vec<String>,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Body {
    V1(V1),
    V2(V2),
    V3(V3),
}

impl Body {
    pub fn pins(&self) -> &[String] {
        match self {
            Body::V1(b) => &b.pins,
            Body::V2(b) => &b.pins,
            Body::V3(b) => &b.pins,
        }
    }

    fn to_value(&self) -> Value {
        match self {
            Body::V1(b) => serde_json::to_value(b),
            Body::V2(b) => serde_json::to_value(b),
            Body::V3(b) => serde_json::to_value(b),
        }
        .expect("serialize body")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub version: Version,
    pub generation: u64,
    pub incarnation: u64,
    pub body: Body,
}

impl Document {
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "chain": "pins",
            "version": tag_of(self.version),
            "generation": self.generation,
            "incarnation": self.incarnation,
            "body": self.body.to_value(),
        }))
        .expect("serialize document")
    }

    fn from_value(v: &Value, version: Version) -> Result<Self, Error> {
        let bad = |what: &str| Error::MalformedDocument(what.to_string());
        let obj = v.as_object().ok_or_else(|| bad("not an object"))?;
        for k in obj.keys() {
            if !["chain", "version", "generation", "incarnation", "body"]
                .contains(&k.as_str())
            {
                return Err(bad("unknown field"));
            }
        }
        let generation = obj
            .get("generation")
            .and_then(Value::as_u64)
            .ok_or_else(|| bad("generation"))?;
        let incarnation = obj
            .get("incarnation")
            .and_then(Value::as_u64)
            .ok_or_else(|| bad("incarnation"))?;
        let body = obj.get("body").ok_or_else(|| bad("body"))?.clone();
        let body = match version {
            PINS_V1 => serde_json::from_value(body).map(Body::V1),
            PINS_V2 => serde_json::from_value(body).map(Body::V2),
            PINS_V3 => serde_json::from_value(body).map(Body::V3),
            _ => unreachable!(),
        }
        .map_err(|e| Error::MalformedDocument(e.to_string()))?;
        Ok(Self {
            version,
            generation,
            incarnation,
            body,
        })
    }

    fn rewrite(&self, to: Version, body: Body) -> Self {
        Self {
            version: to,
            generation: self.generation + 1,
            incarnation: self.incarnation,
            body,
        }
    }

    fn stamp(&self) -> Stamp {
        Stamp {
            store: StoreId(self.incarnation),
            chain: ChainName::PINS,
            revision: Revision(self.generation),
            version: self.version,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PinsCatalog {
    pub backend: BackendSupport,
    pub edges: Vec<Transition<PinsStep>>,
    /// Declared but not implemented.
    pub missing: Vec<PinsStep>,
    pub caps: Vec<(PinsStep, StepCapabilities)>,
}

impl PinsCatalog {
    pub fn two() -> Self {
        Self {
            backend: BackendSupport::FULL,
            edges: vec![Transition {
                previous: PINS_V1,
                version: PINS_V2,
                upgrade: PinsStep::AddNotes,
                downgrade: Some(PinsStep::BackToPlainPins),
            }],
            missing: vec![],
            caps: vec![],
        }
    }

    pub fn three() -> Self {
        let mut c = Self::two();
        c.edges.push(Transition {
            previous: PINS_V2,
            version: PINS_V3,
            upgrade: PinsStep::AddTags,
            downgrade: Some(PinsStep::DropTags),
        });
        c
    }

    pub fn chain(&self) -> Result<Chain<'_, PinsStep>, Error> {
        Chain::new(PINS_V1, &self.edges)
    }

    fn default_caps() -> StepCapabilities {
        StepCapabilities {
            atomic: true,
            lossless: true,
            derivable: false,
        }
    }
}

impl Catalog<PinsStep> for PinsCatalog {
    fn id(&self) -> CatalogId {
        PINS_CATALOG
    }

    fn binding(&self) -> ChainBinding {
        self.chain()
            .expect("valid fixture chain")
            .binding(ChainName::PINS)
    }

    fn backend(&self) -> BackendSupport {
        self.backend
    }

    fn capabilities(&self, s: Step<PinsStep>) -> Option<StepCapabilities> {
        if self.missing.contains(&s.id) {
            return None;
        }
        let known = self.edges.iter().any(|e| {
            (e.previous == s.from && e.version == s.to && e.upgrade == s.id)
                || (e.version == s.from
                    && e.previous == s.to
                    && e.downgrade == Some(s.id))
        });
        if !known {
            return None;
        }
        Some(
            self.caps
                .iter()
                .find(|(id, _)| *id == s.id)
                .map(|(_, c)| *c)
                .unwrap_or_else(Self::default_caps),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recovered {
    Old,
    New,
    Nothing,
}

pub struct LockfileStore {
    root: PathBuf,
    fs: DurableFs,
    catalog: Mutex<PinsCatalog>,
}

impl LockfileStore {
    pub fn open(root: &Path, faults: Faults, catalog: PinsCatalog) -> Self {
        Self {
            root: root.to_path_buf(),
            fs: DurableFs::new(faults),
            catalog: Mutex::new(catalog),
        }
    }

    /// Explicit fixture creation; the only place an incarnation is supplied.
    pub fn create(
        root: &Path,
        faults: Faults,
        catalog: PinsCatalog,
        incarnation: u64,
        version: Version,
        body: Body,
    ) -> Result<Self, Error> {
        let store = Self::open(root, faults, catalog);
        let doc = Document {
            version,
            generation: 1,
            incarnation,
            body,
        };
        store
            .fs
            .write_atomic(&store.doc_path(), &doc.to_bytes(), "create")
            .map_err(PublishError::into_error)?;
        Ok(store)
    }

    pub fn doc_path(&self) -> PathBuf {
        self.root.join("pins.json")
    }

    pub fn intent_path(&self) -> PathBuf {
        self.root.join(".migration.intent")
    }

    pub fn guard_path(&self) -> PathBuf {
        self.root.join(".migration.guard")
    }

    pub fn faults(&self) -> &Faults {
        &self.fs.faults
    }

    pub fn set_catalog(&self, c: PinsCatalog) {
        *self.catalog.lock().unwrap() = c;
    }

    pub fn planning_catalog(&self) -> PinsCatalog {
        let mut c = self.catalog.lock().unwrap().clone();
        c.backend = self.fs.probe();
        c
    }

    fn read_raw(&self) -> Result<(Value, Vec<u8>), Error> {
        let bytes = match std::fs::read(self.doc_path()) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::MissingVersion);
            }
            Err(e) => return Err(e.into()),
        };
        let v = serde_json::from_slice(&bytes)
            .map_err(|_| Error::MalformedVersion)?;
        Ok((v, bytes))
    }

    fn check_chain_name(&self, v: &Value) -> Result<(), Error> {
        match v.get("chain") {
            None => Err(Error::MissingChain),
            Some(Value::String(s)) if s == ChainName::PINS.0 => Ok(()),
            Some(Value::String(_)) => Err(Error::ChainMismatch),
            Some(_) => Err(Error::MalformedChain),
        }
    }

    fn read_document(&self) -> Result<(Document, Vec<u8>), Error> {
        let (raw, bytes) = self.read_raw()?;
        self.check_chain_name(&raw)?;
        let version = pins_version(raw.get("version"))?;
        Ok((Document::from_value(&raw, version)?, bytes))
    }

    fn pending_intent_chain(&self) -> Result<Option<String>, Error> {
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
            Some(Value::String(s)) => Ok(Some(s.clone())),
            _ => Err(Error::MalformedIntent),
        }
    }

    /// Coherent snapshot: the document is replaced atomically, so one read is
    /// one consistent state. Intent is checked first.
    pub fn observe(&self) -> Result<GuardedObservation, Error> {
        if let Some(name) = self.pending_intent_chain()? {
            if name != ChainName::PINS.0 {
                return Err(Error::ChainMismatch);
            }
            return Ok(GuardedObservation::RecoveryRequired);
        }
        Ok(GuardedObservation::Ready(self.read_document()?.0.stamp()))
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
    ) -> Result<Plan<PinsStep>, Error> {
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

    pub fn read_body(&self) -> Result<(Version, Body), Error> {
        let (d, _) = self.read_document()?;
        Ok((d.version, d.body))
    }

    pub fn document_bytes(&self) -> Vec<u8> {
        std::fs::read(self.doc_path()).expect("document")
    }

    pub fn intent_exists(&self) -> bool {
        self.intent_path().exists()
    }

    fn intent_bytes(old: (u64, &str), new: (u64, &str)) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "chain": "pins",
            "old": {"generation": old.0, "fingerprint": old.1},
            "new": {"generation": new.0, "fingerprint": new.1},
        }))
        .unwrap()
    }

    /// A writer that is not a migration: same protocol as a step (refuse a
    /// pending intent, publish its own intent first, then replace).
    pub fn edit_pins(&self, add: &str) -> Result<(), Error> {
        let _guard = self.fs.lock_exclusive(&self.guard_path())?;
        if self.pending_intent_chain()?.is_some() {
            return Err(Error::Dirty);
        }
        let (old, old_bytes) = self.read_document()?;
        let mut body = old.body.clone();
        match &mut body {
            Body::V1(b) => b.pins.push(add.to_string()),
            Body::V2(b) => b.pins.push(add.to_string()),
            Body::V3(b) => b.pins.push(add.to_string()),
        }
        let next = old.rewrite(old.version, body);
        let session_less = Publisher { store: self };
        session_less
            .publish(&old_bytes, old.generation, &next)
            .map_err(|o| match o {
                StepOutcome::NotCommitted(e)
                | StepOutcome::RecoveryRequired(e) => e,
                StepOutcome::Committed => unreachable!(),
            })
    }

    /// Guarded recovery: resolve a pending intent against the live document.
    pub fn recover_under_guard(&self) -> Result<Recovered, Error> {
        let _guard = self.fs.lock_exclusive(&self.guard_path())?;
        let Some(name) = self.pending_intent_chain()? else {
            return Ok(Recovered::Nothing);
        };
        if name != ChainName::PINS.0 {
            return Err(Error::ChainMismatch);
        }
        let intent: Value =
            serde_json::from_slice(&std::fs::read(self.intent_path())?)
                .map_err(|_| Error::MalformedIntent)?;
        let identity = |k: &str| -> Result<(u64, String), Error> {
            let o = intent.get(k).ok_or(Error::MalformedIntent)?;
            Ok((
                o.get("generation")
                    .and_then(Value::as_u64)
                    .ok_or(Error::MalformedIntent)?,
                o.get("fingerprint")
                    .and_then(Value::as_str)
                    .ok_or(Error::MalformedIntent)?
                    .to_string(),
            ))
        };
        let (old, new) = (identity("old")?, identity("new")?);
        let (doc, bytes) =
            self.read_document().map_err(|_| Error::RecoveryBlocked)?;
        let live = (doc.generation, fingerprint(&bytes));
        let which = if live == new {
            Recovered::New
        } else if live == old {
            Recovered::Old
        } else {
            return Err(Error::RecoveryBlocked);
        };
        std::fs::File::open(self.doc_path())?.sync_all()?;
        self.fs.sync_dir(&self.root)?;
        if !self
            .fs
            .remove_and_sync(&self.intent_path(), "intent.clear")?
        {
            return Err(Error::Io("intent removal not synced".into()));
        }
        Ok(which)
    }
}

struct Publisher<'a> {
    store: &'a LockfileStore,
}

impl Publisher<'_> {
    fn publish(
        &self,
        old_bytes: &[u8],
        old_generation: u64,
        next: &Document,
    ) -> Result<(), StepOutcome> {
        let fs = &self.store.fs;
        let next_bytes = next.to_bytes();
        let stage = fs
            .stage(&self.store.doc_path(), &next_bytes, "doc")
            .map_err(StepOutcome::NotCommitted)?;
        let intent = LockfileStore::intent_bytes(
            (old_generation, &fingerprint(old_bytes)),
            (next.generation, &fingerprint(&next_bytes)),
        );
        match fs.write_atomic(&self.store.intent_path(), &intent, "intent") {
            Ok(()) => {}
            Err(PublishError::BeforeRename(e)) => {
                return Err(StepOutcome::NotCommitted(e));
            }
            Err(PublishError::Uncertain(e)) => {
                return Err(StepOutcome::RecoveryRequired(e));
            }
        }
        if let Err(e) = fs.replace(stage, &self.store.doc_path(), "doc") {
            return Err(StepOutcome::RecoveryRequired(e.into_error()));
        }
        match fs.remove_and_sync(&self.store.intent_path(), "intent.clear") {
            Ok(_) => Ok(()),
            Err(e) => Err(StepOutcome::RecoveryRequired(e)),
        }
    }
}

pub struct PinsSession<'a> {
    store: &'a LockfileStore,
    catalog: PinsCatalog,
    _guard: Guard,
}

impl MigrationStore<PinsStep> for LockfileStore {
    type Session<'a> = PinsSession<'a>;

    fn begin_migration(&self) -> Result<PinsSession<'_>, Error> {
        let guard = self.fs.lock_exclusive(&self.guard_path())?;
        Ok(PinsSession {
            store: self,
            catalog: self.planning_catalog(),
            _guard: guard,
        })
    }
}

impl Catalog<PinsStep> for PinsSession<'_> {
    fn id(&self) -> CatalogId {
        self.catalog.id()
    }

    fn binding(&self) -> ChainBinding {
        self.catalog.binding()
    }

    fn backend(&self) -> BackendSupport {
        self.catalog.backend()
    }

    fn capabilities(&self, s: Step<PinsStep>) -> Option<StepCapabilities> {
        self.catalog.capabilities(s)
    }
}

impl MigrationSession<PinsStep> for PinsSession<'_> {
    fn probe(&self) -> BackendSupport {
        self.store.fs.probe()
    }

    fn observe(&self) -> Result<GuardedObservation, Error> {
        self.store.observe()
    }

    fn apply(&mut self, s: Step<PinsStep>) -> StepOutcome {
        let Some(c) = self.capabilities(s) else {
            return StepOutcome::NotCommitted(Error::MissingStep);
        };
        if let Err(e) = check_backend(self.store.fs.probe(), c) {
            return StepOutcome::NotCommitted(e);
        }
        let (old, old_bytes) = match self.store.read_document() {
            Ok((doc, bytes)) if doc.version == s.from => (doc, bytes),
            Ok(_) => {
                return StepOutcome::RecoveryRequired(Error::UnexpectedVersion);
            }
            Err(e) => return StepOutcome::RecoveryRequired(e),
        };
        let body = match (s.id, &old.body) {
            (PinsStep::AddNotes, Body::V1(b)) => Body::V2(V2 {
                pins: b.pins.clone(),
                notes: vec![],
            }),
            (PinsStep::BackToPlainPins, Body::V2(b)) if b.notes.is_empty() => {
                Body::V1(V1 {
                    pins: b.pins.clone(),
                })
            }
            (PinsStep::BackToPlainPins, Body::V2(_)) => {
                return StepOutcome::NotCommitted(Error::MeaningfulNotes);
            }
            (PinsStep::AddTags, Body::V2(b)) => Body::V3(V3 {
                pins: b.pins.clone(),
                notes: b.notes.clone(),
                tags: vec![],
            }),
            (PinsStep::DropTags, Body::V3(b)) if b.tags.is_empty() => {
                Body::V2(V2 {
                    pins: b.pins.clone(),
                    notes: b.notes.clone(),
                })
            }
            (PinsStep::DropTags, Body::V3(_)) => {
                return StepOutcome::NotCommitted(Error::MeaningfulTags);
            }
            _ => {
                return StepOutcome::RecoveryRequired(Error::UnexpectedVersion);
            }
        };
        let next = old.rewrite(s.to, body);
        match (Publisher { store: self.store }).publish(
            &old_bytes,
            old.generation,
            &next,
        ) {
            Ok(()) => StepOutcome::Committed,
            Err(o) => o,
        }
    }
}
