(component
	(type $shared-interface (instance
		(type $dispatch-error (variant
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
		(export "dispatch-error" (type (eq $dispatch-error)))
		(type $dispatch-result (result u32 (error 1)))
		(type $wait (func async (result (tuple string $dispatch-result))))
		(export "wait" (func (type $wait)))
	))
	(import "test:concurrent-shared/root" (instance $shared (type $shared-interface)))
	(alias export $shared "wait" (func $wait))

	(core module $memory
		(memory (export "memory") 1)
		(func (export "realloc") (param i32 i32 i32 i32) (result i32)
			i32.const 256
		)
	)
	(core instance $memory-instance (instantiate $memory))
	(alias core export $memory-instance "memory" (core memory $memory-export))
	(alias core export $memory-instance "realloc" (core func $realloc))
	(core func $lowered-wait
		(canon lower (func $wait) (memory $memory-export) (realloc $realloc))
	)
	(core instance $imports (export "wait" (func $lowered-wait)))
	(core instance $mem (export "memory" (memory $memory-export)))

	(core module $m
		(import "shared" "wait" (func $wait (param i32)))
		(import "mem" "memory" (memory 1))
		(func (export "run") (result i32)
			(call $wait (i32.const 0))
			(i32.load (i32.const 12))
		)
	)
	(core instance $i (instantiate $m
		(with "shared" (instance $imports))
		(with "mem" (instance $mem))
	))
	(func $f async (result u32) (canon lift (core func $i "run")))
	(instance $root (export "run" (func $f)))
	(export "test:concurrent-branch/root" (instance $root))
)
