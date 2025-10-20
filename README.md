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

Run with:

```
   ./client --server hostname:NNNN [--config configdir]  <username>
```

If the `--config` flag is not specified, the client stores its configuration in `~/.mlschat`. The selected directory contains:

- `config.toml`: configuration parameters.
- `client.db`: persisted client keys and chat logs.

The client offers the user a simple command line interface at the bottom of the screen.

The following commands are supported:

- `/create #groupname`: create a group with name `groupname`. Also selects `groupname` to become the default group ("current") to send messages/commands to.
- `/g #groupname`: select `groupname` to become the current group.
- `/invite username`: invite `username` to the current group.
- `/accept #groupname`: accept the invitation to join `groupname`.
- `/decline #groupname`: decline the invitation to join `groupname`.
- `/kick username`: kick `username` away from the current group. After this command, `username` cannot send messages to the group or receive group messages.
- `/mod username`: set `username` as an admin for the group. (Only admins can invite other users and kick them.)
- `/unmod username`: unset `username` as an admin for the group.
- `/groups`: list the groups that the current user is a member of. The text `(admin)` is added next to group names where the current user is an admin.
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

