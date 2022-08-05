# mini-dbg
A small debugger to learn about debuggers.

## Features:
### Breakpoints:
```bash
> b complex_function
Breakpoint 0 at 0x1129 complex_function() in segfault.c, line 1
> b segfault_here
Breakpoint 1 at 0x1144 segfault_here() in segfault.c, line 5
> lsb
Breakpoint 0 at 0x1129 complex_function() in segfault.c, line 1
Breakpoint 1 at 0x1144 segfault_here() in segfault.c, line 5
```

### Continue / Single Step
```bash
> r
0x1129 complex_function() in segfault.c, line 1
⇒       int complex_function(int a, int b) {
            return 2*a + b;
> s
0x1137 complex_function() in segfault.c, line 2
        int complex_function(int a, int b) {
⇒           return 2*a + b;
        }
```

### Backtrace with locals and formals
```bash
> back
Backtrace:
0 0x1137 complex_function() in segfault.c, line 2
    a = 0x1
    b = 0x2
1 0x1198 main() in segfault.c, line 13
    argc =   0x1
    argv =   0x7fffffffee68
    a =      0x1
    b =      0x2
    result = 0x0
```