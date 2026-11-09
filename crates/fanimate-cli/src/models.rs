//! `fanimate-cli models ...`: the model side of the optional AI tools (see
//! `fanimate-ml` and `docs/legal/models.md`). It lists, verifies, installs and
//! removes `.fnmodel` packages, and packs one for a release. Nothing here
//! runs a model or touches the network.

use std::path::PathBuf;
use std::process::ExitCode;

use fanimate_ml::{ModelManifest, ModelStore};

use super::{Args, CmdResult, USAGE};

pub fn command(args: &[String]) -> CmdResult {
    let Some((sub, rest)) = args.split_first() else {
        return Err(USAGE.into());
    };
    match sub.as_str() {
        "list" => list(rest),
        "verify" => verify(rest),
        "install" => install(rest),
        "remove" => remove(rest),
        "pack" => pack(rest),
        other => Err(format!("unknown models command {other}\n{USAGE}")),
    }
}

fn store(a: &Args) -> Result<ModelStore, String> {
    let dir = match a.get::<String>("--folder")? {
        Some(d) => PathBuf::from(d),
        None => fanimate_ml::user_folder()
            .ok_or("this computer names no folder for models: give one with --folder")?,
    };
    Ok(ModelStore::new(dir))
}

/// The keys a package must be signed by: `--trust` (64 hex digits of a
/// public key, more than once is fine). There is no default, so a package is
/// never accepted on a guess.
fn trusted(args: &[String]) -> Result<Vec<[u8; 32]>, String> {
    let keys: Vec<[u8; 32]> = args
        .windows(2)
        .filter(|w| w[0] == "--trust")
        .map(|w| {
            fanimate_ml::key_from_hex(&w[1])
                .ok_or_else(|| format!("--trust takes 64 hex digits, not {}", w[1]))
        })
        .collect::<Result<_, _>>()?;
    if keys.is_empty() {
        return Err("name the public key that signed it with --trust <64 hex digits>".to_owned());
    }
    Ok(keys)
}

fn read(path: &str) -> Result<Vec<u8>, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{path}: {e}"))?;
    if meta.len() > fanimate_ml::package::MAX_PACKAGE_BYTES as u64 {
        return Err(format!("{path}: larger than 1 GiB"));
    }
    std::fs::read(path).map_err(|e| format!("{path}: {e}"))
}

fn describe(m: &ModelManifest) {
    println!("{} {} ({})", m.name, m.version, m.id);
    println!("  task: {}", m.task.label());
    println!("  licence: {} ({})", m.licence, m.licence_file);
    println!(
        "  trained on: {} [{}]",
        m.provenance.training_data,
        m.provenance.consent.name()
    );
    println!("  weights: {} bytes, sha256 {}", m.size, m.sha256);
}

fn list(args: &[String]) -> CmdResult {
    let a = Args::parse(args, &["--folder"])?;
    let store = store(&a)?;
    let found = store.list();
    if found.is_empty() {
        println!("no models installed in {}", store.dir().display());
    }
    for model in &found {
        describe(&model.manifest);
    }
    Ok(ExitCode::SUCCESS)
}

fn verify(args: &[String]) -> CmdResult {
    let a = Args::parse(args, &["--trust"])?;
    let [file] = a.positional.as_slice() else {
        return Err(USAGE.into());
    };
    let keys = trusted(args)?;
    match fanimate_ml::verify(&read(file)?, &keys) {
        Ok(v) => {
            describe(&v.manifest);
            println!("verified: signed by a trusted key, weights match, policy holds");
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            eprintln!("fanimate-cli: refused: {e}");
            Ok(ExitCode::from(1))
        }
    }
}

fn install(args: &[String]) -> CmdResult {
    let a = Args::parse(args, &["--trust", "--folder"])?;
    let [file] = a.positional.as_slice() else {
        return Err(USAGE.into());
    };
    let keys = trusted(args)?;
    let store = store(&a)?;
    let done = store
        .install(&read(file)?, &keys)
        .map_err(|e| format!("refused: {e}"))?;
    describe(&done.manifest);
    println!("installed in {}", done.dir.display());
    Ok(ExitCode::SUCCESS)
}

fn remove(args: &[String]) -> CmdResult {
    let a = Args::parse(args, &["--folder"])?;
    let [id] = a.positional.as_slice() else {
        return Err(USAGE.into());
    };
    store(&a)?.remove(id).map_err(|e| e.to_string())?;
    println!("removed {id}");
    Ok(ExitCode::SUCCESS)
}

/// Pack a model: the manifest (`size` and `sha256` are filled in from the
/// weights), the weights and the licence, signed by the seed in the
/// environment variable named by `--seed-env` (base64, as the update signing
/// secret is kept). The output is refused if the manifest breaks the policy.
fn pack(args: &[String]) -> CmdResult {
    let a = Args::parse(
        args,
        &["--manifest", "--weights", "--licence", "-o", "--seed-env"],
    )?;
    let manifest_path: String = a.get("--manifest")?.ok_or("pack needs --manifest <model.json>")?;
    let weights_path: String = a.get("--weights")?.ok_or("pack needs --weights <file>")?;
    let licence_path: String = a.get("--licence")?.ok_or("pack needs --licence <file>")?;
    let out: String = a.get("-o")?.ok_or("pack needs -o <name.fnmodel>")?;
    let env: String = a
        .get("--seed-env")?
        .ok_or("pack needs --seed-env <NAME of the variable holding the signing seed>")?;
    let seed = std::env::var(&env)
        .ok()
        .and_then(|v| fanimate_ml::seed_from_base64(&v))
        .ok_or_else(|| format!("{env} is not set to a base64 32-byte signing seed"))?;
    // the manifest may leave size and sha256 out: they come from the weights
    let mut value: serde_json::Value = serde_json::from_slice(&read(&manifest_path)?)
        .map_err(|e| format!("{manifest_path}: not JSON: {e}"))?;
    if let Some(map) = value.as_object_mut() {
        map.entry("schema").or_insert(1.into());
        map.entry("size").or_insert(1.into());
        map.entry("sha256").or_insert("0".repeat(64).into());
    }
    let manifest: ModelManifest =
        serde_json::from_value(value).map_err(|e| format!("{manifest_path}: {e}"))?;
    let package = fanimate_ml::pack(manifest, &read(&weights_path)?, &read(&licence_path)?, &seed)
        .map_err(|e| format!("refused: {e}"))?;
    std::fs::write(&out, &package).map_err(|e| format!("{out}: {e}"))?;
    println!(
        "wrote {out} ({} KB), signed by key {}",
        package.len().div_ceil(1024),
        fanimate_ml::hex(&fanimate_ml::public_key(&seed))
    );
    Ok(ExitCode::SUCCESS)
}
