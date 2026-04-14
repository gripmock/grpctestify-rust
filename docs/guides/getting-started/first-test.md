# Your First Test

Create a test, validate it, run it, and inspect output.

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

## 2) Validate before run

```bash
grpctestify check hello_test.gctf
```

## 3) Run it

```bash
grpctestify hello_test.gctf
```

Expected result: the test finishes with a pass status.

## 4) Debug commands

```bash
grpctestify inspect hello_test.gctf --format json
grpctestify explain hello_test.gctf
grpctestify hello_test.gctf --verbose
```

## 5) Common next upgrades

- Add request metadata with `REQUEST_HEADERS`
- Add payload matching with `RESPONSE`
- Add TLS/mTLS settings in `TLS`
- Add per-test execution overrides in `OPTIONS`

Next: [Basic Concepts](basic-concepts).
