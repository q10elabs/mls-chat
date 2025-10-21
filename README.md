# OpenMLS demonstration tools

This project defines a simple server and client tools that demonstrate
how to use OpenMLS to define a simple group chat application.

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
Rust client; `client/node` for a Node.js client).

The general function of the client is to create/connect to a MLS group
and let the user chat with other members of the group:

- the first user that connects to the group creates it and becames its administrator.
- the administrator can "invite" other users. The invitations are delivered to their inbox on the server.
- when another user connects to the server they check if they have a pending invitation for the group first, and use that if there is one.

Run the client with:

```
   ./client --server hostname:NNNN <groupname> <username>
```

The client offers the user a simple command line interface at the bottom of the screen.

The following commands are supported:

- `/invite username`: invite `username` to the current group. Only available if the user created the group (is an administrator).
- `/list`: list the users in the current group. The text `(admin)` is added next to user names who are admin in the group.

When the user enters text that does not start with `/`, this is
interpreted as a message to send to the current group.

Above the command line interface, messages sent to the group(s) and
received from the group(s) are printed from top to bottom (scrolling
up as regular text) with the following format:

```
  #groupname <username>  message...
```

And control messages (from commands) are printed as:

```
  #groupname   action...
```

The state of the client (in particular user keys) are stored in the `~/.mlschat` directory.
