# Real-time Chat

Example test suite location:

```text
examples/basic-examples/real-time-chat/
```

## Example test

```gctf
--- ADDRESS ---
localhost:4770

--- ENDPOINT ---
chat.ChatService/SendMessage

--- REQUEST ---
{
  "room_id": "room_001",
  "user_id": "user_123",
  "message": "Hello"
}

--- ASSERTS ---
.status == "sent"
.message_id | type == "string"
```

## Run

```bash
cd examples/basic-examples/real-time-chat
grpctestify tests/*.gctf
```
