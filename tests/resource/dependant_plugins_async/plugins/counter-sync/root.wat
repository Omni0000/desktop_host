(component
	(core module $shim_module
		(type (func (param i32)))
		(table (export "$imports") 1 1 funcref)
		(export "dtor" (func 0))
		(func (type 0) (param i32)
			local.get 0
			i32.const 0
			call_indirect (type 0)
		)
	)
	(core instance $shim_inst (instantiate $shim_module))
	(alias core export $shim_inst "dtor" (core func $dtor_indirect))

	(type $counter (resource (rep i32) (dtor (func $dtor_indirect))))
	(core func $resource_new (canon resource.new $counter))
	(core func $resource_drop (canon resource.drop $counter))

	(core module $main
		(import "[export]counter" "[resource-new]counter" (func $res_new (param i32) (result i32)))
		(import "[export]counter" "[resource-drop]counter" (func $res_drop (param i32)))
		(memory (export "memory") 1)
		(func (export "[dtor]counter") (param i32))
		(func (export "[constructor]counter") (result i32)
			i32.const 4
			i32.const 42
			i32.store
			i32.const 1
			call $res_new
		)
		(func (export "[method]counter.get-value") (param $rep i32) (result i32)
			local.get $rep
			i32.const 4
			i32.mul
			i32.load
		)
	)

	(core instance $export_counter
		(export "[resource-new]counter" (func $resource_new))
		(export "[resource-drop]counter" (func $resource_drop))
	)
	(core instance $main_inst (instantiate $main
		(with "[export]counter" (instance $export_counter))
	))

	(core module $fixup
		(type (func (param i32)))
		(import "" "dtor" (func (type 0)))
		(import "" "$imports" (table 1 1 funcref))
		(elem (i32.const 0) func 0)
	)
	(alias core export $shim_inst "$imports" (core table $shim_table))
	(alias core export $main_inst "[dtor]counter" (core func $main_dtor))
	(core instance (instantiate $fixup
		(with "" (instance
			(export "dtor" (func $main_dtor))
			(export "$imports" (table $shim_table))
		))
	))

	(alias core export $main_inst "[constructor]counter" (core func $core_ctor))
	(alias core export $main_inst "[method]counter.get-value" (core func $core_get))
	(func $lifted_ctor (result (own $counter))
		(canon lift (core func $core_ctor))
	)
	(func $lifted_make (result (own $counter))
		(canon lift (core func $core_ctor))
	)
	(func $lifted_get (param "self" (borrow $counter)) (result u32)
		(canon lift (core func $core_get))
	)

	(component $shim
		(import "counter-type" (type $ct (sub resource)))
		(import "ctor" (func $ctor (result (own $ct))))
		(import "make" (func $make (result (own $ct))))
		(import "get" (func $get (param "self" (borrow $ct)) (result u32)))
		(export $exp_ct "counter" (type $ct))
		(export "[constructor]counter" (func $ctor) (func (result (own $exp_ct))))
		(export "make-counter" (func $make) (func (result (own $exp_ct))))
		(export "[method]counter.get-value" (func $get) (func (param "self" (borrow $exp_ct)) (result u32)))
	)

	(instance $shim_instance (instantiate $shim
		(with "counter-type" (type $counter))
		(with "ctor" (func $lifted_ctor))
		(with "make" (func $lifted_make))
		(with "get" (func $lifted_get))
	))
	(export "test:async-resource/root" (instance $shim_instance))
)
