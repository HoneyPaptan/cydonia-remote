use cydonia_remote::{
    log::{Log, Replay},
    proto::Change,
};

fn change(n: usize) -> Change {
    Change::ProjectRemoved {
        path: format!("/{n}"),
    }
}

fn seqs(replay: Replay) -> Vec<u64> {
    match replay {
        Replay::Events(events) => events.iter().map(|event| event.seq).collect(),
        Replay::Resync => panic!("expected events, got resync"),
    }
}

#[test]
fn sequence_rises_by_one() {
    let mut log = Log::new(9, 8);
    let got: Vec<u64> = (0..3).map(|n| log.push(change(n)).seq).collect();
    assert_eq!(got, vec![1, 2, 3]);
    assert_eq!(log.seq(), 3);
}

#[test]
fn replays_exactly_what_came_after() {
    let mut log = Log::new(9, 8);
    for n in 0..5 {
        log.push(change(n));
    }
    assert_eq!(seqs(log.since(9, 2)), vec![3, 4, 5]);
    assert_eq!(seqs(log.since(9, 5)), Vec::<u64>::new());
    assert_eq!(seqs(log.since(9, 0)), vec![1, 2, 3, 4, 5]);
}

#[test]
fn fallen_out_of_the_log_means_resync() {
    let mut log = Log::new(9, 3);
    for n in 0..6 {
        log.push(change(n));
    }
    assert_eq!(seqs(log.since(9, 3)), vec![4, 5, 6]);
    assert_eq!(log.since(9, 2), Replay::Resync);
}

#[test]
fn another_epoch_means_resync() {
    let mut log = Log::new(9, 8);
    log.push(change(0));
    assert_eq!(log.since(8, 1), Replay::Resync);
}

#[test]
fn a_sequence_from_the_future_means_resync() {
    let mut log = Log::new(9, 8);
    log.push(change(0));
    assert_eq!(log.since(9, 1842), Replay::Resync);
}

#[test]
fn empty_log_answers_nothing_new() {
    let log = Log::new(9, 8);
    assert_eq!(seqs(log.since(9, 0)), Vec::<u64>::new());
}
