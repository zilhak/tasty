# macOS native HTTP fixture

Run on macOS without input automation. The binary creates only a hidden owned NSWindow, requests no activation, and asserts Prohibited activation policy. It never uses the default WK store or changes system proxy/trust settings.

This standalone package explicitly selects `driver.rs` as its binary and test-harness source. Keeping the driver in this directory under a non-`main.rs` name avoids root Cargo integration-test discovery. Use the fixture manifest below for native builds/tests; root all-target checks do not compile this private driver. Repository source guards still scan it normally.

```sh
cargo test --manifest-path tests/native_macos_http/Cargo.toml --locked -- --nocapture
cargo build --manifest-path tests/native_macos_http/Cargo.toml --locked
python3 tests/native_macos_http/serve.py <target>/debug/native-macos-http --storage-controls
python3 tests/native_macos_http/serve.py <target>/debug/native-macos-http
```

Reuse a single target directory. Set HOME, CFFIXED_USER_HOME and TMPDIR to owned fixture directories for native runs, retaining the existing Rust toolchain/cache configuration during compilation. Serialize foreign native jobs with the host-specific validation lock. The standalone lockfile has 111 registry entries: 93 match existing root versions/checksums, and 18 additive crypto/time dependency variants match the U5 proposed lock (they are absent from the main baseline). Existing root WASI pins are retained: wasip2 1.0.2+wasi-0.2.9 and wit-bindgen 0.51.0. Neither WASI package is in the active macOS graph; the before/after filtered closure is identical (108 reachable packages). The root manifest and module declaration are unchanged.

The default run requires actual authenticated SOCKS, loopback, isolated cookie/storage and selected-clear behavior. Failures are failures, even when a positive baseline or unit checks pass. The driver binds only owned ephemeral loopback listeners, never resolves/connects the requested destination, records the exact child PID/exit and reaps that child. DIRECT_CONNECTION records are an explicit leak detector. The server is a native API control, not the production endpoint/transport or whole-App consent/body integration.

Security tests use real Security.framework evaluation with generated native certificates. They do not fabricate a WK server-trust challenge or prove endpoint NativeTrust ACK. The storage-only run uses loadHTMLString with a fixture base URL; it does not prove HTTP Set-Cookie/POST preservation or actual service-worker/cache eviction. The component remains inactive pending actual routing and fixed-pool integration closure. No macOS automatic key/mouse input is used.

The --domain-control argument runs the non-loopback native HTTP/cookie/POST controls before the required localhost no-bypass assertion. Current Darwin27 behavior fails that assertion: DIRECT_CONNECTION reaches the owned trap. It is an expected blocker receipt, not a passing full routing test.

An independent public Objective-C API control is in proxy_control.m. Compile this validation tool for macOS14 using the selected compatible SDK (the product/adapter fixture still targets macOS11):

```sh
sdk=$(xcrun --sdk macosx --show-sdk-path)
xcrun --sdk macosx clang -isysroot "$sdk" -mmacosx-version-min=14.0 -fobjc-arc -framework Foundation -framework Network -framework AppKit -framework WebKit tests/native_macos_http/proxy_control.m -o <owned-dir>/proxy-control
python3 tests/native_macos_http/serve.py <owned-dir>/proxy-control
FIXTURE_MATCH_HOST=127.0.0.1 python3 tests/native_macos_http/serve.py <owned-dir>/proxy-control
FIXTURE_CONTROL_HOST=remote.invalid python3 tests/native_macos_http/serve.py <owned-dir>/proxy-control
```

The control returns 3 if a successful response comes from the direct trap. A non-loopback remote fixture response returns 0. Do not change system proxy settings to make the tests pass. These fixtures do not establish a supported way to override implicit loopback bypass.
