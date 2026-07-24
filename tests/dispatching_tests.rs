
include!( "test_utils/fixture_linking.rs" );
include!( "test_utils/assert_no_warnings.rs" );

#[path = "dispatching"] mod dispatching {
	mod dependant_plugins_expect_composite ;
	mod dependant_plugins_expect_primitive ;
	mod dependant_error_encoding ;
	mod method_argument_validation ;
	mod method_argument_validation_async ;
	mod function_resource_name_collision ;
	mod duplicate_socket_interfaces ;
	mod dependant_plugins_async ;
	mod single_plugin_async ;
	mod single_plugin_expect_composite ;
	mod single_plugin_expect_primitive ;
	mod single_plugin_void ;
	mod debug_output ;
	mod remap_interface_name ;
	mod remap_single_item_name ;
	mod remap_multiple_item_names ;
	mod remap_interface_and_item_names ;
	mod remap_mixed_plugin_export_names ;
	mod type_erased_binding_cardinality ;
	mod dispatch_serialization ;
	mod concurrent_shared_dependency ;
}
