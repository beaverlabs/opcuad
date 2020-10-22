opcuad is a very simple OPC UA client proxy.

It listens on TCP port 8341 for requests formatted as newline-separated JSON.

There are only two requests supported:
 - {"type": "connect", "host": "localhost", "port": 4855, "endpoint": "/my/UA", "namespace": 2}
 - {"type": "read", "node_ids": ["v1", "v2"]}

The first connects the proxy to an OPC UA server and the second reads some variable values from the connected OPC UA server.
