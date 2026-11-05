//! A script goal through the real binary: kill -9 in the middle of a
//! read-modify-write loop applies each write exactly once.
#[cfg(test)]
mod common;

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use serde_json::{Value, json};

    use crate::common::{Server, TempDir};

    const ITERATIONS: usize = 150;

    /// Each pass reads `count` and `trail`, writes both back in one insert, then
    /// idles 10 ms. Date.now only paces the loop; it never reaches an op argument.
    const SCRIPT: &str = r#"
        const get = (k) => Deno.core.ops.op_rs3_get(k);
        const insert = (o) => Deno.core.ops.op_rs3_insert(JSON.stringify(o));
        for (let i = 0; i < 150; i++) {
            const n = Number(get('count'));
            const t = get('trail');
            insert({ count: String(n + 1), trail: t + i + ',' });
            const end = Date.now() + 10;
            while (Date.now() < end) {}
        }
        get('count')
    "#;

    fn value(server: &Server, key: &str) -> String {
        let body = server.request("GET", &format!("/key/{key}"), "");
        serde_json::from_str(&body).unwrap_or_else(|_| panic!("no value for {key}: {body}"))
    }

    fn goal(server: &Server, id: u64) -> Value {
        serde_json::from_str(&server.request("GET", &format!("/goals/{id}"), "")).unwrap()
    }

    fn prefix(n: usize) -> String {
        (0..n).map(|i| format!("{i},")).collect()
    }

    /// The writes applied so far, read from the trail written with them.
    fn applied(server: &Server) -> usize {
        let trail = value(server, "trail");
        let n = trail.matches(',').count();
        assert_eq!(trail, prefix(n), "a write was repeated or lost");
        let count: usize = value(server, "count").parse().unwrap();
        assert!(count >= n, "count {count} is behind trail {n}");
        n
    }

    fn poll<T>(what: &str, secs: u64, mut f: impl FnMut() -> Option<T>) -> T {
        let deadline = Instant::now() + Duration::from_secs(secs);
        loop {
            if let Some(found) = f() {
                return found;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_script_killed_three_times_applies_each_write_exactly_once() {
        let dir = TempDir::new("scripts-kill");
        let db = dir.0.join("rs3.db");
        let mut server = Server::start(&dir.0, Some(&db), &[]);
        let seed = json!({"count": "0", "trail": "", "job": SCRIPT}).to_string();
        server.request("POST", "/insert", &seed);
        let created: Value = serde_json::from_str(&server.request(
            "POST",
            "/goals",
            r#"{"priority":0,"script":"job"}"#,
        ))
        .unwrap();
        let id = created["id"].as_u64().unwrap();

        let mut last = 0;
        let mut state = 0x2545_F491_4F6C_DD1D_u64;
        for _ in 0..3 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let target = last + 3 + (state % 8) as usize;
            poll("the script to advance", 20, || {
                let n = applied(&server);
                (n >= target).then_some(())
            });
            drop(server);
            server = Server::start(&dir.0, Some(&db), &[]);
            let n = applied(&server);
            assert!(
                n >= target,
                "writes were lost across a kill: {n} < {target}"
            );
            last = n;
        }
        assert_ne!(
            goal(&server, id)["status"],
            "done",
            "finished before the last kill"
        );

        let done = poll("the goal to finish", 30, || {
            let g = goal(&server, id);
            assert_ne!(g["status"], "failed", "{g}");
            (g["status"] == "done").then_some(g)
        });
        assert_eq!(done["output"], "150");
        assert_eq!(value(&server, "count"), "150");
        assert_eq!(value(&server, "trail"), prefix(ITERATIONS));
    }
}
