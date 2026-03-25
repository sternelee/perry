/// Header for heap-allocated strings (mirrors perry_runtime::string::StringHeader).
/// Defined locally to avoid pulling in the entire perry-runtime crate as a dependency,
/// which would cause duplicate symbol errors when linking with libperry_stdlib.a.
#[repr(C)]
pub struct StringHeader {
    /// Length in bytes (not chars - we store UTF-8)
    pub length: u32,
    /// Capacity (allocated space for data)
    pub capacity: u32,
    /// Reference hint for in-place append optimization (0=shared, 1=unique)
    pub refcount: u32,
}
