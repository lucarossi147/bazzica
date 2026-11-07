//! Conformance suite: replays the games recorded from the Python reference engine
//! (`tools/esporta_tracce.py`) and requires byte-identical JSON for every observation,
//! legal-action list, event list and state, plus identical bot decisions.
//!
//! Comparison uses canonical JSON (sorted keys, compact separators), like the Python side.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use bazzica_core::bots::crea_agente;
use bazzica_core::{Azione, GameState, RulesConfig};
use flate2::read::GzDecoder;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

fn cartella() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/tracce")
}

fn canonico<T: Serialize>(x: &T) -> String {
    serde_json::to_string(&serde_json::to_value(x).unwrap()).unwrap()
}

fn impronta<T: Serialize>(x: &T) -> String {
    let h = Sha256::digest(canonico(x).as_bytes());
    h.iter().map(|b| format!("{b:02x}")).collect()
}

/// First path where two JSON values differ (for readable failures).
fn differenza(a: &Value, b: &Value, path: &str) -> Option<String> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for k in x.keys().chain(y.keys()) {
                match (x.get(k), y.get(k)) {
                    (Some(u), Some(v)) => {
                        if let Some(d) = differenza(u, v, &format!("{path}.{k}")) {
                            return Some(d);
                        }
                    }
                    (u, v) => return Some(format!("{path}.{k}: rust={u:?} python={v:?}")),
                }
            }
            None
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => x
            .iter()
            .zip(y)
            .enumerate()
            .find_map(|(i, (u, v))| differenza(u, v, &format!("{path}[{i}]"))),
        _ if a == b => None,
        _ => Some(format!("{path}: rust={a} python={b}")),
    }
}

fn uguale<T: Serialize>(rust: &T, python: &Value, cosa: &str) {
    let r = serde_json::to_value(rust).unwrap();
    if canonico(&r) != canonico(python) {
        let d = differenza(&r, python, "").unwrap_or_else(|| "(serialization only)".into());
        panic!("{cosa} differs: {d}");
    }
}

fn legali_json(st: &GameState, seat: u8) -> Value {
    st.legal_actions(seat)
        .iter()
        .map(|a| {
            let mut d = serde_json::to_value(a).unwrap();
            if let Some(ant) = st.anteprima(a) {
                d["anteprima"] = serde_json::to_value(ant).unwrap();
            }
            d
        })
        .collect()
}

fn hash_osservazioni(st: &GameState) -> Vec<String> {
    (0..4).map(|s| impronta(&st.observation(s))).collect()
}

fn verifica(tr: &Value) {
    let rules: RulesConfig = serde_json::from_value(tr["regole"].clone()).unwrap();
    let mut agenti: Vec<_> = tr["agenti"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| crea_agente(a["tipo"].as_str().unwrap(), a["seed"].as_u64().unwrap()))
        .collect();
    let (mut st, eventi) = GameState::new_game(rules, tr["seed"].as_u64().unwrap(), None);
    let passi = tr["passi"].as_array().unwrap();
    let p0 = &passi[0];
    uguale(&eventi, &p0["eventi"], "initial events");
    assert_eq!(impronta(&st), p0["stato_hash"], "initial state");

    for (k, p) in passi.iter().enumerate().skip(1) {
        let azione: Azione = match p["attore"].as_u64() {
            None => serde_json::from_value(p["azione"].clone()).unwrap(),
            Some(s) => {
                let s = s as u8;
                let oss = st.observation(s);
                uguale(&oss, &p["oss"], &format!("observation at step {k}"));
                assert_eq!(
                    legali_json(&st, s),
                    p["legali"],
                    "legal actions at step {k}"
                );
                let legali = st.legal_actions(s);
                let agente = &mut agenti[usize::from(s)];
                let a = agente.choose_action(&oss, &legali);
                uguale(&a, &p["azione"], &format!("bot choice at step {k}"));
                assert_eq!(agente.rng_stato(), p["rng_bot"], "bot rng at step {k}");
                a
            }
        };
        let ev = st
            .apply(&azione)
            .unwrap_or_else(|e| panic!("step {k}: {e}"));
        uguale(&ev, &p["eventi"], &format!("events at step {k}"));
        if let Some(full) = p.get("stato") {
            uguale(&st, full, &format!("state at step {k}"));
        }
        assert_eq!(impronta(&st), p["stato_hash"], "state hash at step {k}");
        assert_eq!(
            hash_osservazioni(&st),
            p["oss_hash"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_owned())
                .collect::<Vec<_>>(),
            "observation hashes at step {k}"
        );
    }
    assert_eq!(
        serde_json::to_value(st.punteggi).unwrap(),
        tr["finale"]["punteggi"]
    );
    uguale(&st, &tr["finale"]["stato"], "final state");
}

fn carica(file: &str) -> Value {
    let mut s = String::new();
    GzDecoder::new(File::open(cartella().join(file)).unwrap())
        .read_to_string(&mut s)
        .unwrap();
    serde_json::from_str(&s).unwrap()
}

#[test]
fn tutte_le_tracce_python_sono_riprodotte() {
    let indice: Value =
        serde_json::from_reader(File::open(cartella().join("indice.json")).unwrap()).unwrap();
    let files: Vec<String> = indice["tracce"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["file"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(files.len(), 209);
    let n_thread = std::thread::available_parallelism().map_or(4, |n| n.get());
    let fallite: Vec<String> = std::thread::scope(|scope| {
        let handles: Vec<_> = files
            .chunks(files.len().div_ceil(n_thread))
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .filter_map(|f| {
                            let tr = carica(f);
                            std::panic::catch_unwind(|| verifica(&tr)).err().map(|e| {
                                let msg = e
                                    .downcast_ref::<String>()
                                    .cloned()
                                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                                    .unwrap_or_default();
                                format!("{f}: {msg}")
                            })
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect()
    });
    assert!(
        fallite.is_empty(),
        "{} traces differ, e.g.:\n{}",
        fallite.len(),
        fallite[..fallite.len().min(5)].join("\n")
    );
}
