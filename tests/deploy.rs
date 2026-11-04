use agent_rush::deploy::{DeploymentBudget, select_plan};
use serde_json::json;

#[test]
fn rejects_unbounded_cost_lifetime_and_unknown_pricing() {
    let budget = DeploymentBudget::default();
    assert!(budget.check(0.2, 2.0, 0.0).is_ok());
    for (rate, hours, spent) in [
        (f64::NAN, 2.0, 0.0),
        (0.31, 2.0, 0.0),
        (0.2, 2.1, 0.0),
        (0.2, 2.0, 249.9),
        (0.2, -1.0, 0.0),
    ] {
        assert!(budget.check(rate, hours, spent).is_err());
    }
}

#[test]
fn selects_smallest_adequate_available_plan_not_largest_machine() {
    let plans = json!([{"id":"large","vcpu_count":8,"ram":16384,"monthly_cost":96},{"id":"small","vcpu_count":2,"ram":4096,"monthly_cost":24},{"id":"tiny","vcpu_count":1,"ram":1024,"monthly_cost":6}]);
    assert_eq!(
        select_plan(plans.as_array().unwrap(), 2, 4096).unwrap()["id"],
        "small"
    );
    assert!(select_plan(plans.as_array().unwrap(), 16, 32768).is_err());
}
