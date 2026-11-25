//! Host tests of the `__NAME__` service over `FakeCore`: that it starts and asks nothing of the core when it has nothing to do. Add a case for each part method the
//! service overrides (`Records::check`, `Provider::detach`, `Ticker::tick`, ...); the other services' `tests/` show the pattern, and `dkrt_test_support::provider_contract`
//! runs the shared contract suite over a `Recipe` of the service.
use dkrt_abi::provider::Provider;
use dkrt_svc___NAME__::records::__CAMEL__;
use dkrt_test_support::FakeCore;

#[test]
fn the_service_starts_without_asking_the_core() {
    let core = FakeCore::new();
    let mut owner = dkrt_test_support::start_boxed::<__CAMEL__>(&core).expect("start");
    <__CAMEL__ as Provider<FakeCore>>::detach(&mut owner, &core, dkrt_abi::provider::ConsumerId::new(1));
    assert!(core.calls().is_empty());
}
