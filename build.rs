macro_rules! capnp_compile {
    ( $file_path_str:literal ) => {
        match capnpc::CompilerCommand::new()
            .default_parent_module( vec![ "capnp".to_string() ] )
            .file( format!( "capnp/{}", $file_path_str ) )
            .run()
        {
            Err( err ) => panic!(
                "Failed to compile Cap'n Proto file 'capnp/{}': {}",
                $file_path_str,
                err.extra
            ),
            _ => {}
        };
    };
}

fn main() {
    println!( "{}", std::env::var( "OUT_DIR" ).unwrap() );
    capnp_compile!( "common.capnp" );
    capnp_compile!( "manifest.capnp" );
}