
#[macro_export]
macro_rules! test_data_path {
    ($( $segment:expr ),+ $(,)?) => {{
        std::path::PathBuf::from( env!( "CARGO_MANIFEST_DIR" ))
            .join( "tests" )
            $(.join( $segment ))+
    }};
}
