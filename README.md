# OpenMLS demonstration tools

This project contains a simple server and client tools that demonstrate
how to use [OpenMLS](https://openmls.tech/)
([github](https://github.com/openmls/openmls)) to implement a simple
group chat application.

For context, MLS ([Message Layer
Security](https://en.wikipedia.org/wiki/Messaging_Layer_Security)) is
the IETF standard ([RFC
9420](https://www.rfc-editor.org/rfc/rfc9420.html)) derived from the
Signal and Whatsapp end-to-end encryption protocols, with improvements.

**WARNING: The code in this repository is not sufficiently
complete to be correct and safe for use in the same use cases as
Signal. See the section "Limitations / Disclaimers" below.**

## Server usage

The server program is responsible for:

- serving as simple identity directory and storing client keys and
  encrypted state.
- forwarding encrypted messages between clients.

Run with:

```
   ./server [--port NNNN] [--database dbfile.db]
```

When the `--port` flag is not specified, the server runs on port
4000. When the `--database` flag is not specified, a file
`chatserver.db` is used in the current directory.

The server uses SQLite in WAL mode to persist data.

## Client usage

The directory `client` contains client programs (`client/rust` for a
Rust client).

The general function of the client is to create/connect to a MLS group
and let the user chat with other members of the group:

- the first user that connects to the group creates it and becames its administrator.
- the administrator can "invite" other users. The invitations are delivered to their inbox on the server.
- when another user connects to the server they check if they have a pending invitation for the group first, and use that if there is one.

Run the client with:

```
   ./client --server http://hostname:NNNN <groupname> <username>
```

(IMPORTANT: if you wish to try a multi-client conversation on a single
computer, give each client a separate config directory with the
`--config` flag. This is not needed when running the client from
different computers or unix accounts.)

The client offers the user a simple command line interface at the bottom of the screen.

The following commands are supported:

- `/invite username`: invite `username` to the current group.
- `/list`: list the users in the current group.

When the user enters text that does not start with `/`, this is
interpreted as a message to send to the current group.

Above the command line interface, messages sent to the group(s) and
received from the group(s) are printed from top to bottom (scrolling
up as regular text) with the following format:

```
  #groupname <username>  message...
```

The state of the client (in particular user keys) are stored in the
`~/.mlschat` directory. You can override this with `--config`.

## Limitations / disclaimers

At least the following features are REQUIRED for the implementation to
become correct and actually offer the security guarantees of MLS:

- server-side durable buffering of messages, and a "catch up" API for clients to receive all messages they may have missed since they last connected.
  With the current implementation, if a client is not connected when other clients update the group membership, the disconnected client cannot re-sync.

- stronger authentication of clients to the server.
  Currently a malicious client can "take over" the client of an existing user.

The list above is not exhaustive (some additional features may also be required - we did not check).

Additionally, the following desirable features for a chat program are
also missing:

- ability to change the username for a client. Currently the
  communication and membership is keyed on the username string.

- ability to switch groups in the client CLI. Currently the client
  code tracks multiple MLS groups internally but the CLI code can only
  send messages to the latest group joined.

