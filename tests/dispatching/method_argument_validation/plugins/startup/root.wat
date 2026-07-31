(component
	(type $child-interface (instance
		(type $dispatch-error' (variant
			(case "lock-rejected")
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
		(type $get-value (func (result $dispatch-result)))
		(export "get-value" (func (type $get-value)))
	))
	(import "test:child/root" (instance $child (type $child-interface)))
	(alias export $child "dispatch-error" (type $dispatch-error))
	(alias export $child "get-value" (func $get-value))
	(type $dispatch-result (result u32 (error $dispatch-error)))
	(core module $memory
		(memory (export "memory") 1)
		(global $next-allocation (mut i32) (i32.const 256))
		(func (export "realloc") (param i32 i32 i32) (param $new-size i32) (result i32)
			(local $allocation i32)
			global.get $next-allocation
			local.tee $allocation
			local.get $new-size
			i32.add
			global.set $next-allocation
			local.get $allocation
		)
	)
	(core instance $memory (instantiate $memory))
	(alias core export $memory "memory" (core memory $shared-memory))
	(alias core export $memory "realloc" (core func $realloc))
	(core func $lowered-get-value (canon lower (func $get-value)
		(memory $shared-memory)
		(realloc $realloc)
	))
	(core instance $child-imports (export "get-value" (func $lowered-get-value)))
	(core module $adapter
		(import "child" "get-value" (func $get-value (param i32)))
		(func (export "get-value") (result i32)
			i32.const 0
			call $get-value
			i32.const 0
		)
	)
	(core instance $adapter (instantiate $adapter
		(with "child" (instance $child-imports))
	))
	(alias core export $adapter "get-value" (core func $adapted-get-value))
	(func $lifted-get-value (result $dispatch-result) (canon lift
		(core func $adapted-get-value)
		(memory $shared-memory)
		(realloc $realloc)
	))
	(instance $root
		(export "dispatch-error" (type $dispatch-error))
		(export "get-value" (func $lifted-get-value))
	)
	(export "test:method-error/root" (instance $root))
)
