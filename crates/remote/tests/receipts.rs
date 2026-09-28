use cydonia_remote::{
    proto::{Ack, Outcome},
    receipts::Receipts,
};

fn ack(id: &str) -> Ack {
    Ack {
        id: id.into(),
        outcome: Outcome::Accepted,
    }
}

#[test]
fn a_kept_ack_is_found_again() {
    let mut receipts = Receipts::new(4);
    receipts.keep(ack("cmd_1"));
    assert_eq!(receipts.get("cmd_1"), Some(&ack("cmd_1")));
    assert_eq!(receipts.get("cmd_2"), None);
}

#[test]
fn the_first_ack_wins() {
    let mut receipts = Receipts::new(4);
    receipts.keep(ack("cmd_1"));
    receipts.keep(Ack {
        id: "cmd_1".into(),
        outcome: Outcome::Rejected {
            reason: cydonia_remote::proto::Reason::Closed,
        },
    });
    assert_eq!(receipts.get("cmd_1"), Some(&ack("cmd_1")));
}

#[test]
fn oldest_ack_is_forgotten_past_capacity() {
    let mut receipts = Receipts::new(2);
    for id in ["a", "b", "c"] {
        receipts.keep(ack(id));
    }
    assert_eq!(receipts.get("a"), None);
    assert!(receipts.get("b").is_some());
    assert!(receipts.get("c").is_some());
}
