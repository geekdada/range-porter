use range_porter::portset::parse_portset;

#[test]
fn parses_mixed_port_expressions() {
    let ports = parse_portset("80, 443, 10000-10002, 443").expect("parse mixed port expression");
    assert_eq!(ports, vec![80, 443, 10_000, 10_001, 10_002]);
}

#[test]
fn rejects_inverted_ranges() {
    let error = parse_portset("100-90").expect_err("range should be rejected");
    assert_eq!(
        error.to_string(),
        "invalid port range 100-90: start must be <= end"
    );
}

#[test]
fn rejects_zero_ports() {
    let error = parse_portset("0").expect_err("port zero should be rejected");
    assert_eq!(error.to_string(), "port 0 is not a valid listen port");
}
