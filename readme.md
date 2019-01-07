# BRQueue

A fast work processing queue. BRQueue is intended to be put between your work creators and consumers. 
Consumers can grab work from the queue as fast as they can process it, or they can choose to wait 
for work if none is available. 

## Features
What makes BRQueue so special from other queues that you have tried?

* High/Low prioritization for tasks. High prioritization will always be taken before low priority task. 
* Push and pull based task popping
* Capability based task routing, don't send tasks to consumers that can't handle them
* Queue introspection e.g. for better auto-scaling
* Extremely resource efficient

## Clients
You can either make your own client (Description on how TODO)
Or use one of the premade clients. 

* (Go)[https://github.com/zlepper/go-brqueue]
* (C#)[https://github.com/zlepper/brqueue.net]