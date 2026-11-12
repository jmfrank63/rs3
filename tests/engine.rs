//! The candle engine on Qwen3-0.6B: same seed gives the same tokens, even across loads and restores.

#[path = "common/model.rs"]
mod model;

use rs3::model::{Candle, Generation};

const PROMPT: &str = "What is the capital of France? Answer with one word. /no_think";
const CAP: usize = 64;

fn load() -> Result<Candle, String> {
    let (gguf, tokenizer) = model::model_files();
    Candle::load(&gguf, &tokenizer, 4)
}

fn generate(
    candle: &Candle,
    seed: u64,
    committed: &[u32],
    upto: usize,
) -> Result<Vec<u32>, String> {
    let prompt = candle.prompt_tokens(PROMPT)?;
    let mut g = Generation::new(candle, seed, &prompt, committed)?;
    let mut tokens = committed.to_vec();
    while tokens.len() < upto && !tokens.last().is_some_and(|t| candle.is_eos(*t)) {
        tokens.push(g.next_token()?);
    }
    Ok(tokens)
}

#[test]
fn two_loads_agree_and_answer_paris() {
    let (a, b) = (load().unwrap(), load().unwrap());
    let ta = generate(&a, 7, &[], CAP).unwrap();
    let tb = generate(&b, 7, &[], CAP).unwrap();
    assert_eq!(ta, tb);
    assert!(a.is_eos(*ta.last().unwrap()), "no eos in {ta:?}");
    assert!(
        a.render(&ta, true).contains("Paris"),
        "{}",
        a.render(&ta, true)
    );
    assert_eq!(a.model_hash.len(), 64);
    assert_eq!(a.tokenizer_hash.len(), 64);
}

#[test]
fn restore_continues_exactly_where_it_stopped() {
    let c = load().unwrap();
    let whole = generate(&c, 11, &[], CAP).unwrap();
    for k in [1, 3] {
        let k = k.min(whole.len() - 1);
        assert_eq!(
            generate(&c, 11, &whole[..k], CAP).unwrap(),
            whole,
            "restore at {k}"
        );
    }
}

#[test]
fn running_text_never_ends_inside_a_character() {
    let c = load().unwrap();
    let t = generate(&c, 5, &[], 3).unwrap();
    assert!(!c.render(&t, false).contains('\u{fffd}'));
}

#[test]
fn bad_files_are_named_in_the_error() {
    let (gguf, _) = model::model_files();
    let err = Candle::load(&gguf, std::path::Path::new("/nonexistent/tok.json"), 1)
        .err()
        .unwrap();
    assert!(err.contains("/nonexistent/tok.json"), "{err}");
    let err = Candle::load(std::path::Path::new("/nonexistent/m.gguf"), &gguf, 1)
        .err()
        .unwrap();
    assert!(err.contains("/nonexistent/m.gguf"), "{err}");
}

#[test]
fn a_goal_run_on_the_engine_ends_at_eos_and_counts_what_it_sampled() {
    use rs3::goals::{GoalSpec, Goals};
    use rs3::model::Engine;
    use std::{num::NonZeroU32, sync::Arc, time::Duration};

    let dir = std::env::temp_dir().join(format!("rs3-engine-goal-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = rs3::storage::open(&dir.join("rs3.redb")).unwrap();
    let goals = Goals::open(db, NonZeroU32::new(1000).unwrap()).unwrap();
    goals.set_engine(Engine::Candle(Arc::new(load().unwrap())));
    let spec = GoalSpec {
        priority: 0,
        budget: CAP as u64,
        seed: 3,
        prompt: PROMPT.into(),
    };
    let id = goals.create(spec).unwrap();
    let goal = goals.next_goal().unwrap();
    let (done, finished) = std::sync::mpsc::channel();
    let worker = goals.clone();
    std::thread::spawn(move || {
        worker.run(goal);
        let _ = done.send(());
    });
    finished
        .recv_timeout(Duration::from_secs(120))
        .expect("the goal did not finish in time");
    let status = goals.status(id).unwrap().unwrap();
    assert_eq!(status.status, "done");
    assert_eq!(status.reason.as_deref(), Some("eos"));
    assert!(status.output.unwrap().contains("Paris"));
    let sampled = goals.generated_tokens();
    assert!((1..=CAP as u64).contains(&sampled), "{sampled}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn final_text_keeps_only_the_answer() {
    use rs3::model::final_text;
    assert_eq!(final_text("<think>\nhmm\n</think>\n\nAnswer"), "Answer");
    assert_eq!(final_text("<think>a</think>b</think>\n\nlast"), "last");
    assert_eq!(final_text("<think>\nstill thinking"), "");
    assert_eq!(final_text("Just an answer"), "Just an answer");
}

#[test]
fn a_later_turn_is_prompted_with_the_earlier_turns_final_text() {
    use rs3::goals::Goals;
    use rs3::journal::Record;
    use rs3::model::{Engine, final_text};
    use std::{num::NonZeroU32, sync::Arc, time::Duration};

    let dir = std::env::temp_dir().join(format!("rs3-engine-turns-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = rs3::storage::open(&dir.join("rs3.redb")).unwrap();
    let goals = Goals::open(db, NonZeroU32::new(10_000).unwrap()).unwrap();
    let candle = Arc::new(load().unwrap());
    goals.set_engine(Engine::Candle(candle.clone()));
    let session = goals.create_session().unwrap();
    let (p0, p1) = ("Say hi. /no_think", "Say bye. /no_think");
    let mut goal_ids = Vec::new();
    for (n, prompt) in [p0, p1].into_iter().enumerate() {
        let outcome = goals.start_turn(session, n as u64, prompt.into()).unwrap();
        let rs3::sessions::TurnOutcome::New(id) = outcome else {
            panic!("turn {n}: {outcome:?}");
        };
        goal_ids.push(id);
        let goal = goals.next_goal().unwrap();
        let (done, finished) = std::sync::mpsc::channel();
        let worker = goals.clone();
        std::thread::spawn(move || {
            worker.run(goal);
            let _ = done.send(());
        });
        finished
            .recv_timeout(Duration::from_secs(60))
            .expect("the turn did not finish in time");
    }
    let first = goals.status(goal_ids[0]).unwrap().unwrap().output.unwrap();
    assert!(first.contains("<think>"), "{first}");
    let seen = match &goals.journal().read(goal_ids[1]).unwrap()[0] {
        Record::InferenceStart { prompt_tokens, .. } => prompt_tokens.clone(),
        other => panic!("{other:?}"),
    };
    let history = [(p0.to_string(), final_text(&first).to_string())];
    assert_eq!(seen, candle.chat_tokens(&history, p1).unwrap());
    let raw = [(p0.to_string(), first)];
    assert_ne!(seen, candle.chat_tokens(&raw, p1).unwrap());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn history_comes_before_the_prompt_in_the_chat_tokens() {
    let c = load().unwrap();
    let alone = c.prompt_tokens("Say bye.").unwrap();
    assert_eq!(c.chat_tokens(&[], "Say bye.").unwrap(), alone);
    let history = [("Say hi.".to_string(), "Hi!".to_string())];
    let with = c.chat_tokens(&history, "Say bye.").unwrap();
    let earlier = with.len() - alone.len();
    assert!(earlier >= 8, "{with:?}");
    assert_eq!(with[earlier..], alone[..]);
    let exchange = c.chat_tokens(&[], "Say hi.").unwrap();
    assert_eq!(with[..exchange.len() - 3], exchange[..exchange.len() - 3]);
}
