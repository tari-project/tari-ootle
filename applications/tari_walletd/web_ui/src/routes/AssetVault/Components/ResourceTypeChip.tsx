import Chip from "@mui/material/Chip";
import Typography from "@mui/material/Typography";
import { ResourceType } from "@tari-project/ootle-ts-bindings";

interface TypeChipProps {
  type: ResourceType;
}

const colourOptions: Record<ResourceType, string> = {
  Fungible: "rgba(129,59,245, 0.3)",
  NonFungible: "rgba(58,157,160, 0.3)",
  Confidential: "rgba(81,125,137, 0.3)",
  Stealth: "rgba(100,95,236, 0.3)",
};
export default function TypeChip({ type }: TypeChipProps) {
  const label = <Typography variant="label">{type}</Typography>;
  return (
    <Chip label={label} size="small" style={{ background: colourOptions[type], padding: "2px 4px", height: 20, userSelect:'none'}} />
  );
}
