fn main() {
  println!( "{}", std::env::var( "OUT_DIR" ).unwrap() );
  capnpc::CompilerCommand::new()
    .src_prefix("capnp")
    .file("capnp/manifest.capnp")
    .run().expect("schema compiler command");
}