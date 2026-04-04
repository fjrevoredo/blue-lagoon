## What are "Looping LLM Agents"?
Those are regular LLM Agents which not only run on demand by user request, but runs on a "infinite" loop every X minutes or hours.
On each iteration, they perform certain tasks like checking their memory, scheduled tasks, internal instructions and prompts, etc and can decide to take some action or not. This can result or not in sending a message to the user, but it can also mean they will check/do something internal.

## What is their purpose? Why they exist?
The main idea behind looping agents is that they can behave more "human-like" but no only being reactive to user request but also pro-active. This create a completely new layer of possibilities where the agents can ping the user if they detect something wrong and it's especially valuable so far for personal agents. The initial reference implementation is OpenClaw.