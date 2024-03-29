"""Add notes to object

Revision ID: cdf1f60a5f96
Revises: bba1729cfeda
Create Date: 2024-03-28 15:51:07.044224

"""
from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa
import sqlmodel
from sqlalchemy.dialects import postgresql

# revision identifiers, used by Alembic.
revision: str = 'cdf1f60a5f96'
down_revision: Union[str, None] = 'bba1729cfeda'
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    # ### commands auto generated by Alembic - please adjust! ###
    op.add_column('inputobject', sa.Column('notes', sqlmodel.sql.sqltypes.AutoString(), nullable=True))
    op.drop_column('inputobject', 'last_updated')
    # ### end Alembic commands ###


def downgrade() -> None:
    # ### commands auto generated by Alembic - please adjust! ###
    op.add_column('inputobject', sa.Column('last_updated', postgresql.TIMESTAMP(), autoincrement=False, nullable=True))
    op.drop_column('inputobject', 'notes')
    # ### end Alembic commands ###
