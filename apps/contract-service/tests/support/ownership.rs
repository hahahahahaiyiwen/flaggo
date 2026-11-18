// Lifecycle tests supply ownership independently of the local workspace adapter.
// Real ownership races use the production adapter in admission_fencing.rs.
pub struct FixedOwnership;
struct FixedGuard;
impl flaggo_executable_store::AdmissionGuard for FixedGuard {}
impl flaggo_executable_store::AdmissionFence for FixedOwnership {
    fn acquire(
        &self,
        _: &flaggo_executable_store::AnalysisCandidateAdmission,
        ownership: &flaggo_contract::CandidateOwnership,
    ) -> flaggo_executable_store::AuthorityResult<Box<dyn flaggo_executable_store::AdmissionGuard>>
    {
        if ownership.attempt_id != "attempt-1" || ownership.epoch != "1" {
            return Err(flaggo_executable_store::ExecutableStoreError::OwnershipConflict.into());
        }
        Ok(Box::new(FixedGuard))
    }
}
