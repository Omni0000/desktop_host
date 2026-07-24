(component
	(type $child-interface (instance
		(type $dispatch-error' (variant
			(case "invalid-interface-path" string)
			(case "invalid-function" string)
			(case "missing-response")
			(case "runtime-exception" string)
			(case "invalid-argument-list")
			(case "unsupported-type" string)
			(case "resource-table-full")
			(case "resource-handle-conversion-failed")
			(case "invalid-resource-handle")
		))
		(export "dispatch-error" (type (eq $dispatch-error')))
		(type $dispatch-result (result u32 (error 1)))
		(type $get-value (func async (result (tuple string $dispatch-result))))
		(export "get-value" (func (type $get-value)))
	))
	(import "test:async-child/root" (instance $child (type $child-interface)))

	(alias export $child "get-value" (func $get_value))

	(core module $mem_module
		(memory (export "memory") 1)
		(func (export "realloc") (param i32 i32 i32 i32) (result i32)
			i32.const 256
		)
	)
	(core instance $mem_inst (instantiate $mem_module))
	(alias core export $mem_inst "memory" (core memory $shared_mem))
	(alias core export $mem_inst "realloc" (core func $shared_realloc))

	(core func $lowered_get_value
		(canon lower (func $get_value) (memory $shared_mem) (realloc $shared_realloc))
	)
	(core instance $imports_child (export "get-value" (func $lowered_get_value)))
	(core instance $mem_imports (export "memory" (memory $shared_mem)))

	(core module $main_impl
		(import "child" "get-value" (func $get_value (param i32)))
		(import "mem" "memory" (memory 1))

		(func (export "get-primitive") (result i32)
			(call $get_value (i32.const 0))
			(if (result i32) (i32.load (i32.const 8))
				(then (i32.const 0))
				(else (i32.load (i32.const 12)))
			)
		)
	)

	(core instance $main_inst (instantiate $main_impl
		(with "child" (instance $imports_child))
		(with "mem" (instance $mem_imports))
	))

	(alias core export $main_inst "get-primitive" (core func $core_get_primitive))
	(func $lifted_get_primitive async (result u32)
		(canon lift (core func $core_get_primitive))
	)
	(instance $inst (export "get-primitive" (func $lifted_get_primitive)))
	(export "test:dependant-async/root" (instance $inst))
)
