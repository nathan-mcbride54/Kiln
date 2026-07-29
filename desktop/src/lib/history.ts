import {
  ApplicationEventStream,
  type ApplicationEvent,
  type EventEnvelope,
  type EventMetadata,
} from "./events.ts";
import { projectEvents, type TaskProjection } from "./projector.ts";

export type EventPersister = (
  events: readonly EventEnvelope[],
) => Promise<void>;

/// Owns one durable task stream and its deterministic read model.
///
/// New events become visible only after the persistence callback succeeds.
/// A failed write resets the sequencer to the durable tail so retrying cannot
/// create a gap.
export class DurableTaskHistory {
  readonly streamId: string;
  readonly taskId: string;

  #events: readonly EventEnvelope[];
  #projection: TaskProjection;
  #stream: ApplicationEventStream;

  constructor(
    streamId: string,
    taskId: string,
    initialEvents: readonly EventEnvelope[] = [],
  ) {
    this.streamId = streamId;
    this.taskId = taskId;
    this.#events = [];
    this.#projection = projectEvents([]);
    this.#stream = this.createStream([]);
    this.restore(initialEvents);
  }

  get events(): readonly EventEnvelope[] {
    return this.#events;
  }

  get projection(): TaskProjection {
    return this.#projection;
  }

  restore(events: readonly EventEnvelope[]): TaskProjection {
    const projection = projectEvents(events);
    if (
      events.some(
        (event) =>
          event.streamId !== this.streamId ||
          (event.taskId !== undefined && event.taskId !== this.taskId),
      )
    ) {
      throw new Error("Restored events do not belong to this task stream.");
    }

    this.#events = events;
    this.#projection = projection;
    this.#stream = this.createStream(events);
    return projection;
  }

  async append(
    payloads: readonly ApplicationEvent[],
    metadata: EventMetadata,
    persist: EventPersister,
  ): Promise<TaskProjection> {
    const appended = payloads.map((payload) =>
      this.#stream.append(payload, metadata),
    );
    const nextEvents = [...this.#events, ...appended];
    const nextProjection = projectEvents(nextEvents);

    try {
      await persist(appended);
    } catch (error) {
      this.#stream = this.createStream(this.#events);
      throw error;
    }

    this.#events = nextEvents;
    this.#projection = nextProjection;
    return nextProjection;
  }

  private createStream(
    events: readonly EventEnvelope[],
  ): ApplicationEventStream {
    return new ApplicationEventStream(this.streamId, {
      taskId: this.taskId,
      nextSequence: (events.at(-1)?.sequence ?? 0) + 1,
    });
  }
}
