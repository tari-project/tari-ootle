import { useEffect, useState } from "react";
import { parseTimestamp } from "../utils/helpers";

export function useTimeAgo(rawTimestamp: string | null | undefined): string {
  const getTimeAgo = (timestamp: string | null | undefined): string => {
    const date = parseTimestamp(timestamp);
    if (!date) return "";

    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffSeconds = Math.floor(diffMs / 1000);
    const diffMinutes = Math.floor(diffSeconds / 60);
    const diffHours = Math.floor(diffMinutes / 60);
    const diffDays = Math.floor(diffHours / 24);

    if (diffSeconds < 60)
      return `${diffSeconds} sec${diffSeconds !== 1 ? "s" : ""} ago`;
    if (diffMinutes < 60)
      return `${diffMinutes} min${diffMinutes !== 1 ? "s" : ""} ago`;
    if (diffHours < 24)
      return `${diffHours} hour${diffHours !== 1 ? "s" : ""} ago`;
    if (diffDays === 1) return "yesterday";
    if (diffDays < 7) return `${diffDays} day${diffDays !== 1 ? "s" : ""} ago`;

    return date.toLocaleDateString(undefined, {
      year: "numeric",
      month: "short",
      day: "numeric",
    });
  };

  const [display, setDisplay] = useState(() => getTimeAgo(rawTimestamp));

  useEffect(() => {
    const update = () => setDisplay(getTimeAgo(rawTimestamp));
    update();

    const interval = setInterval(update, 60 * 1000);
    return () => clearInterval(interval);
  }, [rawTimestamp]);

  return display;
}
