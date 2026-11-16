//! WSL boundary validation and refresh coalescing.
use tervix_application::wsl::*;

struct Fake(bool);
impl Discovery for Fake {
    fn refresh(&mut self) -> bool {
        self.0 = true;
        true
    }
    fn poll(&mut self) -> Option<DiscoveryState> {
        self.0.then(|| {
            self.0 = false;
            DiscoveryState::Ready(Vec::new())
        })
    }
}
#[test]
fn catalog_loading_coalesces_requests_and_empty_is_not_unavailable() {
    let mut catalog = DistributionCatalog::new(Box::new(Fake(false)));
    assert_eq!(catalog.state(), &DiscoveryState::Idle);
    assert!(catalog.refresh());
    assert_eq!(catalog.state(), &DiscoveryState::Loading);
    assert!(!catalog.refresh());
    assert!(catalog.poll());
    assert_eq!(catalog.state(), &DiscoveryState::Ready(Vec::new()));
    assert!(!catalog.poll());
    assert!(catalog.refresh());
}
#[test]
fn launch_rejects_windows_cwd_and_nul_but_preserves_direct_arguments() {
    for cwd in [
        "C:\\Users\\me",
        "relative",
        "\\\\server\\share",
        "/home/\0bad",
    ] {
        assert!(
            Launch {
                distribution: None,
                linux_cwd: Some(cwd.into()),
                argv: vec![]
            }
            .validate()
            .is_err()
        );
    }
    for name in ["", "--Ubuntu", "Ubuntu\ncmd", " Ubuntu"] {
        assert!(DistributionName::new(name).is_err());
    }
    assert!(
        Launch {
            distribution: None,
            linux_cwd: Some("/home/a b".into()),
            argv: vec!["/bin/echo".into(), "a; $(cmd)".into()]
        }
        .profile()
        .is_ok()
    );
}
