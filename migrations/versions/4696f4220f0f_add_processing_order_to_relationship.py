"""Add processing order to relationship

Revision ID: 4696f4220f0f
Revises: cdf1f60a5f96
Create Date: 2024-04-02 15:39:32.657477

"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa
import sqlmodel


# revision identifiers, used by Alembic.
revision: str = "4696f4220f0f"
down_revision: Union[str, None] = "cdf1f60a5f96"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    # ### commands auto generated by Alembic - please adjust! ###

    # Set existing fields to default value of 0
    op.add_column(
        "inputobjectassociations",
        sa.Column(
            "processing_order",
            sa.Integer(),
            nullable=False,
            server_default="0",
        ),
    )
    # ### end Alembic commands ###


def downgrade() -> None:
    # ### commands auto generated by Alembic - please adjust! ###
    op.drop_column("inputobjectassociations", "processing_order")
    # ### end Alembic commands ###
