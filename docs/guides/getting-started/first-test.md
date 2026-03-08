# Your First Test

## 1) Create a file

Create `hello_test.gctf`:

```gctf
--- ADDRESS ---
localhost:4770

--- ENDPOINT ---
hello.HelloService/SayHello

--- REQUEST ---
{
  "name": "World"
}

--- ASSERTS ---
.message == "Hello, World!"
```

## 2) Run it

```bash
grpctestify hello_test.gctf
```

## 3) Helpful commands

```bash
grpctestify check hello_test.gctf
grpctestify inspect hello_test.gctf --format json
grpctestify explain hello_test.gctf
```
