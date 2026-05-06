use std::os::fd::AsFd;

use memmap2::{MmapMut, MmapOptions};
use tempfile::tempfile;
use wayland_client::{
    Dispatch, QueueHandle,
    protocol::{wl_buffer, wl_shm, wl_shm_pool},
};

use crate::error::{AppError, Result};

#[derive(Debug)]
pub struct ShmBuffer {
    pub wl_buffer: wl_buffer::WlBuffer,
    pub data: MmapMut,
    pub width: i32,
    pub height: i32,
    #[allow(dead_code)]
    pub stride: i32,
    #[allow(dead_code)]
    pub size: usize,
    #[allow(dead_code)]
    pub format: wl_shm::Format,
}

impl ShmBuffer {
    pub fn create<D>(
        shm: &wl_shm::WlShm,
        qh: &QueueHandle<D>,
        format: wl_shm::Format,
        width: i32,
        height: i32,
        stride: i32,
    ) -> Result<Self>
    where
        D: Dispatch<wl_shm_pool::WlShmPool, ()> + Dispatch<wl_buffer::WlBuffer, ()> + 'static,
    {
        let size = (stride as usize)
            .checked_mul(height as usize)
            .ok_or_else(|| AppError::buffer_allocation("buffer size overflow"))?;

        let file = tempfile().map_err(AppError::buffer_allocation)?;
        file.set_len(size as u64)
            .map_err(AppError::buffer_allocation)?;

        // Safety: the file is newly created, resized to exactly `size`, and
        // remains alive until the mapping and wl_shm pool creation complete.
        let data = unsafe {
            MmapOptions::new()
                .len(size)
                .map_mut(&file)
                .map_err(AppError::buffer_allocation)?
        };

        let pool = shm.create_pool(file.as_fd(), size as i32, qh, ());
        let wl_buffer = pool.create_buffer(0, width, height, stride, format, qh, ());
        pool.destroy();

        Ok(Self {
            wl_buffer,
            data,
            width,
            height,
            stride,
            size,
            format,
        })
    }
}

impl Drop for ShmBuffer {
    fn drop(&mut self) {
        self.wl_buffer.destroy();
    }
}
