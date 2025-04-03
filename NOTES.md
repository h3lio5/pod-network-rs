### Implementation Notes
The project is divided into Replica and Client code. 

### Client
- each connection stream is a separate tokio task
- a central transaction disperser sends the tx to each of the replica connection handlers, which then unicast it to the replica, get back the vote and send it to the client's aggregator service.
- if threshold 'weight' votes on the tx, then it is considered 'confirmed'.
- add backlogging capability
- bootstrap / init sync and compute the state
- vote aggregation

### Replica
- maintain point-to-point connection with the client after the client sends a 'connection request'. Additionally, unicast the replication log (at the time of connection request) to the client.
- round / view change messages must also be sent to the clients.