# HTTP transport bridge

The `tasty-ipc` HTTP leaf defines an opt-in Control-frame family separate from file HTML transfer. It does not enable or advertise a remote HTTP viewer. The host exports the leaf and activates ordered dispatch only when its server and native routes support the negotiated capabilities.

`HttpRequest` and `HttpReply` preserve the six-field transfer identity, source generation and server-issued response serial. Only encoded chunks cross this wire. Complete carries both encoded and decoded byte lengths and SHA256 identities. Clients assemble and verify the encoded body, decode under a separate bounded worker quota, verify the decoded identity and scan before exposing a body to native rendering. Native must receive the same immutable encoded representation with matching status and end-to-end headers.

Both directions validate the complete serialized frame before enqueue. Header order, duplicate names and non-UTF8 value bytes are retained. Metadata, header counts/bytes, methods, chunk windows, decoded base64 and portable lengths are bounded. HTTP DTOs deliberately omit Debug implementations for request URLs, headers and payloads. Failure reasons must be sanitized by their producer; validation bounds their size and rejects control characters.

The HTTP and HTTPS capabilities additionally require the existing HTML transfer capability. Existing file wire messages and old-peer behavior remain unchanged. The server descriptor must not advertise an executor merely because these types compile. Native parsing, TLS exceptions, cache retirement and browser consent are separate integration obligations.
