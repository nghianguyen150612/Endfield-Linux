//! Explicit opt-in only. This test is ignored by default and preserves data.
//! See docs/WINE-QUALIFICATION.md. No game is ever invoked.
use endfield_unix_prefix_management::authentic_qualification::{
    QualificationRequest, QualificationResult,
};
use endfield_unix_storage::AppPaths;
use std::path::PathBuf;

#[test]
#[ignore = "authentic Wine requires authenticated artifact, headless isolation and exact reviewed approval"]
fn authentic_wine_initialization_opt_in() {
    let request = QualificationRequest {
        source_paths: AppPaths::discover().expect("source AppPaths"),
        pin_id: std::env::var("ENDFIELD_AUTHENTIC_WINE_PIN").expect("exact pin"),
        pin_revision: std::env::var("ENDFIELD_AUTHENTIC_WINE_REVISION")
            .expect("exact pin revision")
            .parse()
            .expect("numeric pin revision"),
        artifact_sha256: std::env::var("ENDFIELD_AUTHENTIC_WINE_ARTIFACT_SHA256")
            .expect("exact artifact digest"),
        test_root: PathBuf::from(
            std::env::var_os("ENDFIELD_AUTHENTIC_WINE_TEST_ROOT")
                .expect("explicit private test root"),
        ),
    };
    match std::env::var("ENDFIELD_AUTHENTIC_WINE_STAGE").as_deref() {
        Ok("prepare") => match request.prepare() {
            Ok(plan) => eprintln!("AWAITING_APPROVAL, authentic execution NOT_RUN. Plan {plan}"),
            Err(error) => panic!("PREFLIGHT_BLOCKED, authentic execution NOT_RUN. {error}"),
        },
        Ok("execute") => {
            let opt_in = std::env::var("ENDFIELD_AUTHENTIC_WINE_OPT_IN").expect("explicit opt-in");
            let plan =
                std::env::var("ENDFIELD_AUTHENTIC_WINE_PLAN_APPROVAL").expect("reviewed plan");
            let result = request
                .execute(&opt_in, &plan)
                .expect("qualification blocked; inspect preserved evidence");
            assert_eq!(
                result,
                QualificationResult::Qualified,
                "inspect preserved outcome"
            );
        }
        _ => panic!("set ENDFIELD_AUTHENTIC_WINE_STAGE to prepare or execute"),
    }
}
