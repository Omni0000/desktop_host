#[macro_export] macro_rules! extract_wasm_args {
    ( $caller:expr, $ptr:expr, $size:expr ) => {
        crate::startup::WasmSendContext::read_data( $caller, crate::startup::WasmMemorySegment::new_unchecked( $ptr, $size ).as_send())
    };
}

#[macro_export] macro_rules! encapsulate_wasm_response {
    ( $caller:expr, $data:expr ) => {
        $caller.send_data( &$data )
    }
}