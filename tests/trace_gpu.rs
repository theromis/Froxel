use super::*;

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    fn raw() -> RawWaker {
        fn clone(_: *const ()) -> RawWaker {
            raw()
        }
        fn noop(_: *const ()) {}
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
        RawWaker::new(std::ptr::null(), &VTABLE)
    }
    let waker = unsafe { Waker::from_raw(raw()) };
    let mut future = std::pin::pin!(future);
    let mut cx = Context::from_waker(&waker);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::sleep(std::time::Duration::from_millis(1)),
        }
    }
}

#[test]
fn the_trace_shader_builds_on_a_gpu() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let Ok(adapter) = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
    else {
        return;
    };
    let (device, _queue) =
        block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("device");
    let _tracer = Tracer::new(&device);
}

#[test]
fn a_frame_fits_when_its_pixel_buffer_and_textures_do() {
    let limits = wgpu::Limits::default();
    assert!(frame_fits([1024, 500], &limits).is_ok());
    // 64 bytes a pixel: the App Store header is about 386 MiB.
    let err = frame_fits([3840, 1646], &limits).unwrap_err();
    assert!(err.contains("3840x1646"), "{err}");
    let roomy = wgpu::Limits {
        max_buffer_size: 4 << 30,
        max_storage_buffer_binding_size: 4 << 30,
        ..wgpu::Limits::default()
    };
    assert!(frame_fits([3840, 1646], &roomy).is_ok());
    assert!(frame_fits([5244, 2950], &roomy).is_ok());
    // The smaller of the two buffer limits wins.
    let bound = wgpu::Limits {
        max_storage_buffer_binding_size: 128 << 20,
        ..roomy.clone()
    };
    assert!(frame_fits([3840, 1646], &bound).is_err());
    let wide = frame_fits([9000, 100], &roomy).unwrap_err();
    assert!(wide.contains("texture"), "{wide}");
}
