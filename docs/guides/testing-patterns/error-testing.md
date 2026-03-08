# Error Testing

Use `ERROR` when a call is expected to fail.

## Basic error expectation

```gctf
--- ENDPOINT ---
user.UserService/GetUser

--- REQUEST ---
{ "user_id": "missing" }

--- ERROR ---
{
  "code": 5,
  "message": "User not found"
}
```

## Error with assertions

```gctf
--- ERROR with_asserts=true ---
{
  "code": 3,
  "message": "Invalid input"
}

--- ASSERTS ---
.code == 3
.message | contains("Invalid")
```

Notes:
- `RESPONSE` and `ERROR` cannot be in the same test file
- `ERROR` supports `with_asserts=true|false`
