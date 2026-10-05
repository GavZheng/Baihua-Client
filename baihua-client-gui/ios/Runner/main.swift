// The UIKit entry contract of winit 0.30 (the event loop eframe builds on):
//   * the event loop must be created and run on the MAIN thread
//     (winit's `EventLoop::new` asserts exactly that), and
//   * `UIApplicationMain` must NOT have run before winit starts -- winit
//     calls it itself (default principal class, no app delegate; its iOS
//     backend drives everything through UIApplication notifications).
// The previous shape here (Swift calling UIApplicationMain on the main
// thread plus Rust looping on a helper thread) contradicts both assertions
// and could never start the UI. The Swift side is now a single synchronous
// hand-over to Rust, and `ios/build-ipa.sh` pins the binary's recorded SDK
// below 26 (vtool) so iOS 26/27 keeps the legacy application lifecycle
// instead of hard-trapping "no scene lifecycle adoption" -- the crash the
// developer handed over (iPad15,7 / iPhone OS 27.0, LiveContainer host).
baihua_ios_main()
