import { Chip, Tooltip, Stack } from '@mui/material';
import { useTimeAgo } from '../../hooks/useTimeAgo';
import { formatTimestamp, isTimestampNew } from '../../utils/helpers';

function TimeChip({ timestamp }: { timestamp: string }) {
  const timeAgo = useTimeAgo(timestamp);
  const showNew = isTimestampNew(timestamp);

  return (
    <Tooltip
      title={`Created at: ${formatTimestamp(timestamp)}` || ''}
      placement="top"
      arrow
    >
      <Stack direction="row" spacing={1} alignItems="center">
        <Chip
          label={timeAgo}
          color="default"
          size="small"
          variant="filled"
          sx={{ padding: '2px 4px 0px 4px', marginTop: '4px' }}
        />
        {showNew && (
          <Chip
            label="New"
            color="success"
            size="small"
            variant="filled"
            sx={{ padding: '2px 4px 0px 4px', marginTop: '4px' }}
          />
        )}
      </Stack>
    </Tooltip>
  );
}

export default TimeChip;
