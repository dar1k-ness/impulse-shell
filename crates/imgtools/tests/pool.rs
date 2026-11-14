//! Parallel image work leaves no threads behind (rayon's global pool would park one per CPU
//! for the rest of the process), yet back-to-back jobs share one pool.
use rayon::prelude::*;
use std::time::Duration;

fn threads() -> Vec<String> {
    std::fs::read_dir("/proc/self/task")
        .unwrap()
        .filter_map(|e| std::fs::read_to_string(e.ok()?.path().join("comm")).ok())
        .map(|s| s.trim_end().to_string())
        .collect()
}

struct OnExit(std::sync::mpsc::Sender<()>);

impl Drop for OnExit {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

thread_local! {
    static EXIT: std::cell::RefCell<Option<OnExit>> = const { std::cell::RefCell::new(None) };
}

fn job() -> (u64, Vec<String>, std::thread::ThreadId) {
    imgtools::pool::scoped(|| {
        // A worker shows up in /proc before it has set its name; once every worker has reached
        // the broadcast, all of them have.
        let all = std::sync::Barrier::new(rayon::current_num_threads());
        let names = rayon::broadcast(|_| {
            all.wait();
            threads()
        })
        .swap_remove(0);
        let sum: u64 = (0..1_000_000u64).into_par_iter().sum();
        let nested = imgtools::pool::scoped(rayon::current_num_threads);
        assert_eq!(
            nested,
            rayon::current_num_threads(),
            "nested call reuses the pool"
        );
        (
            sum,
            names,
            rayon::broadcast(|_| std::thread::current().id())[0],
        )
    })
}

#[test]
fn pool_is_shared_by_a_burst_and_exits_when_idle() {
    // The pool's idle clock moves only when the test says so.
    imgtools::pool::test_clock::freeze();
    let (sum, during, w1) = job();
    assert_eq!(sum, 499_999_500_000);
    let n = std::thread::available_parallelism().unwrap().get();
    assert_eq!(
        during
            .iter()
            .filter(|t| t.starts_with("img") && *t != "img-idle")
            .count(),
        n,
        "{during:?}"
    );

    let (.., w2) = job();
    assert_eq!(w1, w2, "a job within IDLE of the last reuses its threads");

    // Each worker reports its exit: a thread-local's destructor runs as its thread ends.
    let (exit_tx, exits) = std::sync::mpsc::channel();
    let (workers, w2b) = imgtools::pool::scoped(|| {
        let ids = rayon::broadcast(|_| {
            EXIT.with(|e| *e.borrow_mut() = Some(OnExit(exit_tx.clone())));
            std::thread::current().id()
        });
        (ids.len(), ids[0])
    });
    assert_eq!(w2, w2b, "a job within IDLE of the last reuses its threads");
    drop(exit_tx);
    assert_eq!(workers, n);

    imgtools::pool::test_clock::advance(imgtools::pool::IDLE - Duration::from_millis(1));
    assert!(imgtools::pool::alive(), "wound down early");
    imgtools::pool::test_clock::advance(Duration::from_millis(1));
    assert!(!imgtools::pool::alive(), "still up after IDLE");
    assert_eq!(exits.iter().count(), workers, "every worker exited");

    let (.., w3) = job();
    assert_ne!(w1, w3, "a job after the pool wound down starts a fresh one");
}
