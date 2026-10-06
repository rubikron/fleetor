import { useState } from "react";
import { MessageFeed } from "./MessageFeed";
import { EventFeed } from "./EventFeed";
import type { MessageEvent, CommandEvent, FleetEvent } from "../fleet/types";
import type { TaskFeedEntry } from "../fleet/useFleet";

type FeedTab = "messages" | "activity";

interface FeedViewProps {
  messages: MessageEvent[];
  commands: CommandEvent[];
  feed: FleetEvent[];
  taskFeed: TaskFeedEntry[];
}

export function FeedView({ messages, commands, feed, taskFeed }: FeedViewProps) {
  const [tab, setTab] = useState<FeedTab>("messages");
  return (
    <div className="feed-view">
      <div className="feed-view__tabs" role="tablist">
        <button
          role="tab"
          aria-selected={tab === "messages"}
          className={`feed-view__tab ${tab === "messages" ? "feed-view__tab--active" : ""}`}
          onClick={() => setTab("messages")}
        >
          Messages
        </button>
        <button
          role="tab"
          aria-selected={tab === "activity"}
          className={`feed-view__tab ${tab === "activity" ? "feed-view__tab--active" : ""}`}
          onClick={() => setTab("activity")}
        >
          Activity
        </button>
      </div>
      <div className={`feed-view__panel ${tab === "messages" ? "" : "is-hidden"}`}>
        <MessageFeed messages={messages} commands={commands} />
      </div>
      <div className={`feed-view__panel ${tab === "activity" ? "" : "is-hidden"}`}>
        <EventFeed feed={feed} tasks={taskFeed} />
      </div>
    </div>
  );
}
