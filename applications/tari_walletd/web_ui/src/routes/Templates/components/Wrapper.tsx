import AccountPicker from "@components/AccountPicker";
import { StyledPaper } from "@components/StyledComponents";
import Grid from "@mui/material/Grid";

interface WrapperProps {
  children: React.ReactNode;
}
export default function Wrapper({ children }: WrapperProps) {
  return (
    <Grid size={12}>
      <StyledPaper>
        <AccountPicker />
        {children}
      </StyledPaper>
    </Grid>
  );
}
