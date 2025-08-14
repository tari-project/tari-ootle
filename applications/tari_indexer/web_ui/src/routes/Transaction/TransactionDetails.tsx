import PageHeading from '../../Components/PageHeading';
import Grid from '@mui/material/Grid';
import { StyledPaper } from '../../Components/StyledComponents';
import { useParams } from 'react-router-dom';
import Result from './components/Result';

function TransactionDetailsLayout() {
  const { transaction_id } = useParams();
  return (
    <>
      <Grid item sm={12} md={12} xs={12}>
        <PageHeading>Transaction Details</PageHeading>
      </Grid>
      <Grid item sm={12} md={12} xs={12}>
        <StyledPaper>
          <Result transaction_id={transaction_id || '0'} />
        </StyledPaper>
      </Grid>
    </>
  );
}

export default TransactionDetailsLayout;
