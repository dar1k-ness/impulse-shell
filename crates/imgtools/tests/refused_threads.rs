//! The system refuses the pool's threads (the limit on threads is reached): image work runs
//! on the calling thread instead of panicking, for that call only.

use rayon::prelude::*;
use std::sync::mpsc;

#[test]
fn scoped_work_runs_on_the_calling_thread_when_threads_are_refused() {
    // The refusal holds for the thread that asks for it. This one, started before, still gets
    // threads: when told to, it starts the shared pool.
    let (start, started) = (mpsc::channel::<()>(), mpsc::channel::<()>());
    let other = std::thread::spawn(move || {
        start.1.recv().unwrap();
        imgtools::pool::scoped(|| ());
        started.0.send(()).unwrap();
    });
    // The pool, once started, stays for the rest of the test.
    imgtools::pool::test_clock::freeze();

    testenv::refuse_new_threads();
    let me = std::thread::current().id();
    let (sum, ids) = imgtools::pool::scoped(|| {
        let sum: u64 = (0..100_000u64).into_par_iter().sum();
        let ids: Vec<_> = (0..64)
            .into_par_iter()
            .map(|_| std::thread::current().id())
            .collect();
        (sum, ids)
    });
    assert_eq!(sum, 4_999_950_000);
    assert!(ids.iter().all(|&t| t == me));
    assert_eq!(
        rayon::current_thread_index(),
        None,
        "the call left this thread a pool's worker"
    );
    // Again: the first call left this thread able to run the next one.
    let again: u64 = imgtools::pool::scoped(|| (0..1000u64).into_par_iter().sum());
    assert_eq!(again, 499_500);

    // A panic of the work reaches the caller, and the thread can run the next call.
    let panicked = std::panic::catch_unwind(|| {
        imgtools::pool::scoped(|| {
            (0..64)
                .into_par_iter()
                .for_each(|i| assert_ne!(i, 7, "seven"))
        })
    });
    let panic = panicked.unwrap_err();
    assert!(
        panic
            .downcast_ref::<String>()
            .is_some_and(|m| m.contains("seven")),
        "the work's own panic"
    );
    assert_eq!(rayon::current_thread_index(), None);
    let borrowed = vec![1u64, 2, 3];
    assert_eq!(
        imgtools::pool::scoped(|| borrowed.par_iter().sum::<u64>()),
        6
    );

    // Threads are to be had again: the pool is there, and this thread's work runs on it.
    assert!(!imgtools::pool::alive());
    start.0.send(()).unwrap();
    started.1.recv().unwrap();
    assert!(imgtools::pool::alive());
    let names: Vec<String> = imgtools::pool::scoped(|| {
        (0..64)
            .into_par_iter()
            .map(|_| {
                std::thread::current()
                    .name()
                    .unwrap_or_default()
                    .to_string()
            })
            .collect()
    });
    assert!(
        names.iter().all(|n| n.starts_with("img")),
        "ran off the pool: {names:?}"
    );
    other.join().unwrap();
}
