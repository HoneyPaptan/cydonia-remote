use super::*;

#[test]
fn every_switch_is_named_once_and_read_back_by_its_name() {
    let keys: Vec<String> = Switch::all().map(Switch::key).collect();
    let mut unique = keys.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), keys.len());
    assert!(keys.contains(&"mcp.serve".to_owned()));
    assert!(keys.contains(&"feature.sessions".to_owned()));
    for switch in Switch::all() {
        assert_eq!(Switch::parse(&switch.key()), Some(switch));
    }
    assert_eq!(Switch::parse("mcp.everything"), None);
}

#[test]
fn a_laptop_setup_is_adopted_switch_by_switch() {
    let (mut features, mut mcp) = (Features::default(), Mcp::default());
    mcp.serve = false;
    features.tables = true;
    let laptop = setup(&features, &mcp, Some("http://127.0.0.1:7457/mcp".into()));

    let (mut phone_features, mut phone_mcp) = (Features::default(), Mcp::default());
    phone_mcp.serve = true;
    adopt(&laptop, &mut phone_features, &mut phone_mcp);

    assert!(!phone_mcp.serve);
    assert!(phone_features.tables);
    assert_eq!(laptop_mcp_url().as_deref(), Some("http://127.0.0.1:7457/mcp"));
}
