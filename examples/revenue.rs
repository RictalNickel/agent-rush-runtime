// Standard-library-only Rust task fixture. Run ONLY through `agent-rush sandbox`.
fn main() {
    let csv = "region,revenue\nNorth,1200\nSouth,850\nNorth,400\nWest,950\n";
    let mut totals = std::collections::BTreeMap::<String, u64>::new();
    for line in csv.lines().skip(1) {
        let (region, value) = line.split_once(',').expect("two columns");
        *totals.entry(region.into()).or_default() += value.parse::<u64>().expect("integer revenue");
    }
    assert_eq!(totals["North"], 1600);
    assert_eq!(totals.values().sum::<u64>(), 3400);
    let result = format!(
        r#"{{"total":{},"regions":{{"North":{},"South":{},"West":{}}},"assertions_passed":true}}"#,
        totals.values().sum::<u64>(),
        totals["North"],
        totals["South"],
        totals["West"]
    );
    std::fs::write("/work/artifacts/result.json", result).unwrap();
    println!("Revenue verified: North 1600, South 850, West 950. Total 3400.");
}
