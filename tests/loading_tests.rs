
include!( "test_utils/test_data_path.rs" );

#[path = "loading"] mod loading {
    mod dependant_plugins ;
    mod single_plugin ;
}
