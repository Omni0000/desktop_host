
include!( "test_utils/test_data_path.rs" );

#[path = "dispatching"] mod dispatching {
    mod dependant_plugins_expect_composite ;
    mod dependant_plugins_expect_primitive ;
    mod single_plugin_expect_composite ;
    mod single_plugin_expect_primitive ;
}