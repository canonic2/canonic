//! Pinned real React acceptance assets shared by library and installed fixtures.
pub const HTML: &str = r#"<!doctype html><title>Acme React</title><div id=root></div><script src=/react-fixture.js></script><script src=/react-dom-fixture.js></script><script>
function AcmeCounter(){const [count,setCount]=React.useState(0);return React.createElement('button',{id:'count',onClick:()=>setCount(value=>value+1)},'Count '+count);}
const AcmeLazy=React.lazy(()=>new Promise(resolve=>globalThis.acmeResolve=()=>resolve({default:()=>React.createElement('p',null,'Acme loaded')})));
ReactDOM.createRoot(document.getElementById('root')).render(React.createElement('main',null,React.createElement(AcmeCounter),React.createElement(React.Suspense,{fallback:React.createElement('p',null,'Acme loading')},React.createElement(AcmeLazy))));
</script>"#;
pub fn asset(dom: bool) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    let root = std::env::var_os("TESTRUN_FRAMEWORK_FIXTURE")
        .expect("Select the pinned private React fixture directory");
    let (name, digest) = if dom {
        (
            "testrun-react-dom-18.3.1.js",
            "f9044a5e9c39db8bb1a204dff924e526ec0a621e695bb69de1035811be8709e4",
        )
    } else {
        (
            "testrun-react-18.3.1.js",
            "28348fef6cb0ed8b2ceeb22deaf824428fd13875d84c73d38f77dd216fc24e7f",
        )
    };
    let bytes = std::fs::read(std::path::Path::new(&root).join(name)).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        digest,
        "React fixture bytes changed"
    );
    bytes
}
