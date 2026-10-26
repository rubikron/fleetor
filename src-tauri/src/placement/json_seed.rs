//! The JSON-document seeder a harness keeping one JSON config file shares.

use super::harness::{Harness, Seed};

/// **The shared JSON read-modify-write behind [`Harness::seed_config_dir`]** — checkpoint 4
/// and checkpoint 14's trust half, in one write, for a harness that records both
/// in one JSON document.
///
/// Give `dir` the keys an interactive `claude` needs to reach its prompt in `cwd`,
/// without touching anything else that is already there.
///
/// **Which file, which keys, and whether to merge come off the spec** (WP-25).
///
/// **Merge, never clobber**, where [`ConfigDir::seed_merges`](super::harness::ConfigDir::seed_merges)
/// says so. The dir may hold a real `machineID`, cached experiment data, and —
/// after the operator switches targets — trust flags for other project paths.
/// Overwriting the file would work exactly once.
///
/// **Re-run this for every pane cwd whenever the target changes.** The trust
/// record is keyed by absolute project path, not global; Phase 0 bisected this. It
/// is the single most likely way to reintroduce L1 immediately after fixing it,
/// which is why [`place`](super::place) calls it on the placement path for every
/// pane rather than once at the target picker.
pub(super) fn seed_json_document(harness: &dyn Harness, seed: &Seed<'_>) -> Result<(), String> {
    let (dir, cwd) = (seed.config_dir, seed.cwd);
    let spec = harness.spec();
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("create config dir {}: {e}", dir.display()))?;
    let file = dir.join(spec.config_dir.seed_file);

    let mut root = match (spec.config_dir.seed_merges, std::fs::read_to_string(&file)) {
        (true, Ok(text)) => {
            serde_json::from_str::<serde_json::Value>(&text).unwrap_or_else(|_| json_object())
        }
        _ => json_object(),
    };
    if !root.is_object() {
        root = json_object();
    }

    let object = root.as_object_mut().expect("just ensured it is an object");
    for key in spec.config_dir.seed_keys {
        object.insert((*key).into(), true.into());
    }

    // Checkpoint 14's key, **asked of the harness rather than canonicalized here**
    // (WP-25, M23). The trust flag and the context gauge now take the same answer
    // from the same harness instead of agreeing only because both happened to call
    // one shared function — which is what has to be true before a second harness
    // gives a different answer.
    let project = harness.project_key(cwd);
    let projects =
        object.entry("projects").or_insert_with(json_object).as_object_mut().ok_or_else(|| {
            format!("existing \"projects\" in {} is not an object", spec.config_dir.seed_file)
        })?;
    let entry =
        projects.entry(project).or_insert_with(json_object).as_object_mut().ok_or_else(|| {
            format!(
                "existing project entry in {} is not an object",
                spec.project_identity.trust_file
            )
        })?;
    for key in spec.project_identity.trust_keys {
        entry.insert((*key).into(), true.into());
    }

    // Write via a sibling temp file: a half-written seed file is a pane that boots
    // into onboarding, which is the failure this whole function exists to prevent.
    let tmp = file.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(&root).map_err(|e| format!("encode config: {e}"))?;
    std::fs::write(&tmp, text).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &file).map_err(|e| format!("install {}: {e}", file.display()))
}

fn json_object() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}
