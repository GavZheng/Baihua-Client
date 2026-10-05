/* The one Rust symbol the Swift runner needs: it blocks the calling thread
   with winit's event loop and must therefore run OFF the main thread. */
void baihua_ios_main(void);
