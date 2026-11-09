//! The model and data policy (decision D2-6), as code.
//!
//! **Permissive redistribution licences only; documented provenance;
//! commissioned or donated data for any model Fanimate trains; no scraped
//! art.** The lists live in `models/policy.json`, one file read both here and
//! by `scripts/models.py` (the release check), so the two cannot drift.
//!
//! A licence is an SPDX expression. It passes when one alternative of every
//! `OR` is made only of licences on the allow-list (`AND` needs all of
//! them). A licence that restricts how the weights may be used (a
//! non-commercial term, a Responsible-AI licence, a community licence with a
//! use policy) is not refused as "wrong" but flagged: those need a lawyer's
//! decision, so they are never allowed by default, and the message says so.
//! Anything the list does not know is refused, never guessed at.

use serde::Deserialize;

const POLICY_JSON: &str = include_str!("../../../models/policy.json");

/// How the training data was gathered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Consent {
    /// Made for Fanimate, paid for, with written consent.
    Commissioned,
    /// Given to the project with written consent.
    Donated,
    /// In the public domain.
    PublicDomain,
    /// Under an open licence that allows this use (such data's own licence is recorded in `notes`).
    PermissiveOpen,
    /// Not known: refused.
    Unknown,
}

impl Consent {
    pub fn name(self) -> &'static str {
        match self {
            Self::Commissioned => "commissioned",
            Self::Donated => "donated",
            Self::PublicDomain => "public-domain",
            Self::PermissiveOpen => "permissive-open",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Deserialize)]
struct Lists {
    allowed_licences: Vec<String>,
    needs_a_lawyer: Vec<String>,
    consents: Vec<String>,
}

fn lists() -> &'static Lists {
    static LISTS: std::sync::OnceLock<Lists> = std::sync::OnceLock::new();
    // the file is part of the program: a damaged one is a build error caught by the tests
    LISTS.get_or_init(|| {
        serde_json::from_str(POLICY_JSON).unwrap_or(Lists {
            allowed_licences: Vec::new(),
            needs_a_lawyer: Vec::new(),
            consents: Vec::new(),
        })
    })
}

/// The licences the policy allows.
pub fn allowed_licences() -> &'static [String] {
    &lists().allowed_licences
}

/// The consent kinds the policy accepts.
pub fn accepted_consents() -> &'static [String] {
    &lists().consents
}

/// Check an SPDX licence expression. `Err` says why not, in a sentence.
pub fn check_licence(expression: &str) -> Result<(), String> {
    let text = expression.trim();
    if text.is_empty() || text.len() > 200 {
        return Err("the licence is missing, or is not a licence expression".to_owned());
    }
    let tokens = tokenise(text)?;
    let mut at = 0;
    let verdict = parse_or(&tokens, &mut at)?;
    if at != tokens.len() {
        return Err(format!(
            "\"{text}\" is not a licence expression Fanimate can read"
        ));
    }
    match verdict {
        Verdict::Pass => Ok(()),
        Verdict::Fail(why) => Err(why),
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Open,
    Close,
    And,
    Or,
    Id(String),
}

fn tokenise(text: &str) -> Result<Vec<Token>, String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut Vec<Token>| {
        if !word.is_empty() {
            out.push(match word.as_str() {
                "AND" => Token::And,
                "OR" => Token::Or,
                _ => Token::Id(std::mem::take(word)),
            });
            word.clear();
        }
    };
    for c in text.chars() {
        match c {
            '(' | ')' => {
                flush(&mut word, &mut out);
                out.push(if c == '(' { Token::Open } else { Token::Close });
            }
            c if c.is_whitespace() => flush(&mut word, &mut out),
            c if c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '+' | '_') => word.push(c),
            _ => {
                return Err(format!(
                    "\"{text}\" is not a licence expression Fanimate can read"
                ));
            }
        }
    }
    flush(&mut word, &mut out);
    if out.len() > 40 {
        return Err("the licence expression is too long".to_owned());
    }
    Ok(out)
}

enum Verdict {
    Pass,
    Fail(String),
}

fn parse_or(t: &[Token], at: &mut usize) -> Result<Verdict, String> {
    let mut best = parse_and(t, at)?;
    while t.get(*at) == Some(&Token::Or) {
        *at += 1;
        let next = parse_and(t, at)?;
        // either alternative will do: one that passes saves the expression
        if matches!(best, Verdict::Fail(_)) && matches!(next, Verdict::Pass) {
            best = next;
        }
    }
    Ok(best)
}

fn parse_and(t: &[Token], at: &mut usize) -> Result<Verdict, String> {
    let mut result = parse_atom(t, at)?;
    while t.get(*at) == Some(&Token::And) {
        *at += 1;
        let next = parse_atom(t, at)?;
        // every part is needed: the first failure is the answer
        if matches!(result, Verdict::Pass) {
            result = next;
        }
    }
    Ok(result)
}

fn parse_atom(t: &[Token], at: &mut usize) -> Result<Verdict, String> {
    match t.get(*at) {
        Some(Token::Open) => {
            *at += 1;
            let inner = parse_or(t, at)?;
            if t.get(*at) != Some(&Token::Close) {
                return Err("a bracket in the licence is not closed".to_owned());
            }
            *at += 1;
            Ok(inner)
        }
        Some(Token::Id(id)) => {
            *at += 1;
            Ok(judge(id))
        }
        _ => Err("the licence expression is incomplete".to_owned()),
    }
}

fn judge(id: &str) -> Verdict {
    let lists = lists();
    if lists.allowed_licences.iter().any(|a| a == id) {
        return Verdict::Pass;
    }
    let lower = id.to_ascii_lowercase();
    if lists
        .needs_a_lawyer
        .iter()
        .any(|term| lower.contains(&term.to_ascii_lowercase()))
    {
        return Verdict::Fail(format!(
            "\"{id}\" limits how the weights may be used (non-commercial, research-only or use-restricted terms): it needs a lawyer's decision before any model under it can be added"
        ));
    }
    Verdict::Fail(format!(
        "\"{id}\" is not on the list of permissive licences ({})",
        lists.allowed_licences.join(", ")
    ))
}

/// Whether `consent` is one the policy accepts (never \`unknown\`).
pub fn consent_ok(consent: Consent) -> bool {
    lists().consents.iter().any(|c| c == consent.name())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The registry of models a release carries holds only models the policy allows: the
    /// same rule `scripts/models.py --check` applies in the release script.
    #[test]
    fn every_model_in_the_registry_meets_the_policy() {
        let registry: serde_json::Value = serde_json::from_str(include_str!("../../../models/registry.json"))
            .expect("registry.json is JSON");
        let models = registry["models"].as_array().expect("a models list");
        let mut seen = std::collections::BTreeSet::new();
        for m in models {
            let id = m["id"].as_str().unwrap_or("?");
            assert!(seen.insert(id.to_owned()), "{id} is listed twice");
            let licence = m["licence"].as_str().unwrap_or("");
            assert!(
                check_licence(licence).is_ok(),
                "{id}: {:?}",
                check_licence(licence)
            );
            let prov = &m["provenance"];
            assert_eq!(prov["scraped_art"], false, "{id}: scraped art");
            let consent = prov["consent"].as_str().unwrap_or("");
            assert!(
                accepted_consents().iter().any(|c| c == consent),
                "{id}: consent {consent}"
            );
            assert!(
                prov["training_data"].as_str().unwrap_or("").len() >= 20,
                "{id}: provenance text"
            );
        }
    }

    #[test]
    fn the_policy_file_is_read_and_holds_what_the_decision_says() {
        assert!(allowed_licences().iter().any(|l| l == "MIT"));
        assert!(allowed_licences().iter().any(|l| l == "Apache-2.0"));
        assert!(
            !allowed_licences()
                .iter()
                .any(|l| l.contains("GPL") || l.contains("NC"))
        );
        assert_eq!(accepted_consents().len(), 4);
        assert!(!accepted_consents().iter().any(|c| c == "unknown"));
        assert!(consent_ok(Consent::Commissioned) && consent_ok(Consent::Donated));
        assert!(!consent_ok(Consent::Unknown));
    }

    #[test]
    fn permissive_licences_pass_alone_and_in_expressions() {
        for ok in [
            "MIT",
            "Apache-2.0",
            "MIT OR Apache-2.0",
            "Apache-2.0 AND MIT",
            "(MIT OR GPL-3.0-only) AND BSD-3-Clause",
            "  CC-BY-4.0 ",
            "GPL-3.0-only OR MIT",
        ] {
            assert!(check_licence(ok).is_ok(), "{ok}: {:?}", check_licence(ok));
        }
    }

    #[test]
    fn copyleft_unknown_and_malformed_licences_are_refused_with_a_reason() {
        for bad in [
            "GPL-3.0-only",
            "AGPL-3.0-or-later",
            "LicenseRef-custom",
            "MIT AND GPL-2.0-only",
            "proprietary",
        ] {
            let why = check_licence(bad).unwrap_err();
            assert!(
                why.contains("permissive") || why.contains("lawyer"),
                "{bad}: {why}"
            );
        }
        for bad in [
            "",
            "   ",
            "MIT OR",
            "(MIT",
            "MIT)",
            "MIT; rm -rf",
            "AND",
            &"MIT OR ".repeat(60),
        ] {
            assert!(check_licence(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn licences_that_restrict_use_are_flagged_for_a_lawyer_not_allowed() {
        for restricted in [
            "CC-BY-NC-4.0",
            "OpenRAIL-M",
            "CreativeML-OpenRAIL-M",
            "BigScience-RAIL-1.0",
            "Llama-3-Community",
            "Research-Only-1.0",
            "Gemma",
        ] {
            let why = check_licence(restricted).unwrap_err();
            assert!(why.contains("lawyer"), "{restricted}: {why}");
        }
        // an alternative that is permissive still saves a dual-licensed model
        assert!(check_licence("OpenRAIL-M OR Apache-2.0").is_ok());
        assert!(check_licence("OpenRAIL-M AND Apache-2.0").is_err());
    }
}
