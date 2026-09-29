mod common;

use common::{header, key, permission, project, session};
use cydonia_remote::{
    hub::Hub,
    mirror::Mirror,
    notice::{Kind, Notices, between},
    proto::Status,
};

fn mirror_with(header: cydonia_remote::proto::SessionHeader) -> Mirror {
    let mut view = session(Vec::new());
    view.header = header;
    Mirror {
        projects: vec![project("/work", &[("one", view)])],
        ..Mirror::default()
    }
}

fn kinds(before: &Mirror, after: &Mirror) -> Vec<Kind> {
    between(before, after).into_iter().map(|raised| raised.kind).collect()
}

#[test]
fn a_turn_that_ends_raises_done() {
    let before = mirror_with(header(Status::Working));
    let after = mirror_with(header(Status::Idle));

    assert_eq!(kinds(&before, &after), vec![Kind::Done]);
}

#[test]
fn a_session_that_stays_idle_raises_nothing() {
    let idle = mirror_with(header(Status::Idle));

    assert_eq!(kinds(&idle, &idle), vec![]);
}

#[test]
fn a_new_permission_request_raises_approval_with_its_command() {
    let before = mirror_with(header(Status::Working));
    let mut asking = header(Status::WaitingForPermission);
    asking.permission = Some(permission(3));
    let after = mirror_with(asking);

    let raised = between(&before, &after);

    assert_eq!(raised.len(), 1);
    assert_eq!(raised[0].kind, Kind::Approval);
    assert_eq!(raised[0].body, "cargo test --workspace");
    assert_eq!(raised[0].key, key("/work", "one"));
}

#[test]
fn the_same_permission_request_is_raised_once() {
    let mut asking = header(Status::WaitingForPermission);
    asking.permission = Some(permission(3));
    let held = mirror_with(asking);

    assert_eq!(kinds(&held, &held), vec![]);
}

#[test]
fn a_lost_agent_raises_lost() {
    let before = mirror_with(header(Status::Working));
    let after = mirror_with(header(Status::Lost));

    assert_eq!(kinds(&before, &after), vec![Kind::Lost]);
}

#[test]
fn a_session_that_appears_already_working_raises_nothing() {
    let after = mirror_with(header(Status::Idle));

    assert_eq!(kinds(&Mirror::default(), &after), vec![]);
}

#[test]
fn the_hub_keeps_notices_in_order_and_hands_out_only_newer_ones() {
    let hub = Hub::new(1);
    hub.publish(mirror_with(header(Status::Working)));
    let start = hub.notices_after(u64::MAX).latest;
    hub.publish(mirror_with(header(Status::Idle)));
    hub.publish(mirror_with(header(Status::Working)));
    hub.publish(mirror_with(header(Status::Idle)));

    let Notices { latest, notices } = hub.notices_after(start);

    assert_eq!(notices.len(), 2);
    assert!(notices[0].id < notices[1].id);
    assert_eq!(latest, notices[1].id);
    assert_eq!(hub.notices_after(latest).notices, vec![]);
}

